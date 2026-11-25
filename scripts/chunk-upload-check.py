"""Chunked (TUS) upload checks against a running FileHub on :8090.
Small and large files, resume after a break, new version, upload link,
and the same rules as a regular upload (checked before any byte is sent)."""
import base64, hashlib, http.cookiejar, json, os, time, urllib.error, urllib.request, uuid
B = os.environ.get("FH_BASE", "http://localhost:8090") + "/fh/api"
R = []
def check(name, ok, detail=""):
    R.append(ok); print("PASS" if ok else "FAIL", name, "—", detail)

class C:
    def __init__(s, email=None, pw=None):
        s.cj = http.cookiejar.CookieJar(); s.op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(s.cj))
        if email: s.req("POST", "/auth/login", body=json.dumps({"email": email, "password": pw}).encode(), h={"content-type": "application/json"})
    def req(s, m, p, body=None, h=None):
        url = p if p.startswith("http") else B + p
        try: r = s.op.open(urllib.request.Request(url, data=body, method=m, headers=h or {})); return r.status, r.read(), r.headers
        except urllib.error.HTTPError as e: return e.code, e.read(), e.headers
    def j(s, m, p, obj=None):
        c, b, _ = s.req(m, p, json.dumps(obj).encode() if obj is not None else None, {"content-type": "application/json"} if obj is not None else {})
        try: return c, json.loads(b or b"null")
        except Exception: return c, b

def meta(d): return ",".join(f"{k} {base64.b64encode(str(v).encode()).decode()}" for k, v in d.items())
T = {"Tus-Resumable": "1.0.0"}

def start(cl, base, size, md):
    c, b, h = cl.req("POST", base, b"", {**T, "Upload-Length": str(size), "Upload-Metadata": meta(md)})
    return c, b, (base.rsplit("/", 1)[0] + "/" + h["Location"]) if c == 201 else None, h

def send(cl, url, data, chunk, start_at=0, stop_after=None):
    off, sent, last = start_at, 0, None
    while off < len(data):
        part = data[off:off + chunk]
        c, b, h = cl.req("PATCH", url, part, {**T, "Upload-Offset": str(off), "Content-Type": "application/offset+octet-stream"})
        if c != 204: return c, b, h
        off = int(h["Upload-Offset"]); last = h; sent += 1
        if stop_after and sent >= stop_after: return 204, b"", h
    return 204, b"", last

admin, anong, viewer = C("admin@acme.go.th", "admin123"), C("anong@acme.go.th", "anong123"), C("viewer@acme.go.th", "viewer123")
tag = time.strftime("%H%M%S")
# Own basket with the rules under test: allowed types, a 19 MB limit, a required field.
_, nb = admin.j("POST", "/systems", {"name": f"Chunk rules {tag}", "bucket": f"chunk-{tag}", "tone": "slate"})
SYS, BIG = nb["id"], "sys_dms"
_, st = admin.j("GET", f"/baskets/{SYS}/settings")
st["policy"].update({"allowed_ext": ["pdf", "docx", "xlsx", "txt"], "max_file_bytes": 20_000_000})
st["fields"] = [{"key": "fiscal_year", "label": "Fiscal year", "field_type": "text", "required": True, "choices": [], "searchable": True, "show_in_list": True}]
admin.j("PUT", f"/baskets/{SYS}/settings", st)
MB = 1024 * 1024

# 1 small file — one chunk
small = b"%PDF-1.4 small " + os.urandom(3000)
c, _, url, _ = start(anong, "/uploads", len(small), {"filename": f"small-{tag}.pdf", "system_id": SYS})
c2, _, h = send(anong, url, small, 5 * MB)
fid = h and h.get("Upload-File-Id")
cd, body, _ = anong.req("GET", f"/files/{fid}/download") if fid else (0, b"", None)
check("Small file in one chunk", c == 201 and c2 == 204 and body == small, f"create {c} · file id {fid} · bytes match {body == small}")

# 2 large file — 40 MB in 8 MB chunks
big = os.urandom(40 * MB)
t = time.time()
c_lim, b_lim, _, _ = start(anong, "/uploads", len(big), {"filename": f"big-{tag}.pdf", "system_id": SYS})
check("Refused before upload: larger than the basket allows", c_lim == 413, f"{c_lim} {b_lim[:70]!r}")
c, _, url, _ = start(anong, "/uploads", len(big), {"filename": f"big-{tag}.pdf", "system_id": BIG})
c2, _, h = send(anong, url, big, 8 * MB)
fid_big = h and h.get("Upload-File-Id")
cd, body, _ = anong.req("GET", f"/files/{fid_big}/download")
check("Large file (40 MB) in 8 MB chunks", c2 == 204 and hashlib.sha256(body).digest() == hashlib.sha256(big).digest(),
      f"{len(big)//MB} MB · {time.time()-t:.1f}s · sha256 match")

# 3 resume after a break
data = os.urandom(20 * MB)
c, _, url, _ = start(anong, "/uploads", len(data), {"filename": f"resume-{tag}.pdf", "system_id": BIG})
send(anong, url, data, 4 * MB, stop_after=2)                     # connection "drops" after 8 MB
ch, _, hh = anong.req("HEAD", url, None, T)
at = int(hh["Upload-Offset"])
c2, _, h = send(anong, url, data, 4 * MB, start_at=at)
cd, body, _ = anong.req("GET", f"/files/{h.get('Upload-File-Id')}/download")
check("Resume from where it stopped", at == 8 * MB and body == data, f"server had {at//MB} MB · finished · bytes match {body == data}")

# 4 wrong offset → 409, nothing doubled
c, _, url, _ = start(anong, "/uploads", 10, {"filename": f"x-{tag}.pdf", "system_id": SYS})
c1, _, _ = anong.req("PATCH", url, b"12345", {**T, "Upload-Offset": "0", "Content-Type": "application/offset+octet-stream"})
c2, _, _ = anong.req("PATCH", url, b"12345", {**T, "Upload-Offset": "0", "Content-Type": "application/offset+octet-stream"})
check("Chunk sent twice is refused (409), not doubled", c1 == 204 and c2 == 409, f"{c1} then {c2}")
anong.req("DELETE", url, None, T)

# 5 rules checked BEFORE any byte: type, viewer, unknown status, other person's session
c_type, b_type, _, _ = start(anong, "/uploads", 100, {"filename": f"bad-{tag}.exe", "system_id": SYS})
c_view, _, _, _ = start(viewer, "/uploads", 100, {"filename": "v.pdf", "system_id": SYS})
c_st, b_st, _, _ = start(anong, "/uploads", 100, {"filename": "s.pdf", "system_id": SYS, "status": "NoSuchStatus"})
c, _, url, _ = start(anong, "/uploads", 10, {"filename": f"own-{tag}.pdf", "system_id": SYS})
c_other, _, _ = admin.req("HEAD", url, None, T)
check("Refused before upload: type not allowed", c_type == 400, f"{c_type} {b_type[:80]!r}")
check("Refused before upload: viewer can't upload", c_view == 403, str(c_view))
check("Refused before upload: status not in basket", c_st == 400, f"{c_st}")
check("Someone else's upload can't be seen", c_other == 404, str(c_other))
anong.req("DELETE", url, None, T)

# 6 new version via chunks
v2 = b"%PDF-1.4 version two " + os.urandom(6 * MB)
c, b, url, _ = start(anong, "/uploads", len(v2), {"filename": f"small-{tag}.pdf", "purpose": "version", "file_id": fid, "note": "chunked v2"})
c2, _, h = send(anong, url, v2, 2 * MB)
cf, f = anong.j("GET", f"/files/{fid}")
cd, body, _ = anong.req("GET", f"/files/{fid}/download")
check("New version through chunks", c2 == 204 and f.get("version") == 2 and body == v2, f"version {f.get('version')} · bytes match {body == v2}")
c_vv, _, _, _ = start(viewer, "/uploads", 10, {"filename": "x.pdf", "purpose": "version", "file_id": fid})
check("Viewer can't send a new version", c_vv == 403, str(c_vv))

# 7 upload link — anonymous, chunked
c, link = anong.j("POST", "/upload-links", {"system_id": SYS, "title": f"Chunk link {tag}", "expires_in_days": 1})
tok = link["token"]
pub = C()
c_noname, b_noname, _, _ = start(pub, f"/upload/{tok}/chunks", 100, {"filename": "a.pdf"})
data = b"%PDF-1.4 link " + os.urandom(9 * MB)
c_req, b_req, _, _ = start(pub, f"/upload/{tok}/chunks", 100, {"filename": "a.pdf", "submitter_name": "Somchai"})
check("Upload link: required basket fields checked before upload", c_req == 400, f"{c_req} {b_req[:60]!r}")
_, st = anong.j("GET", f"/baskets/{SYS}/settings")
req = {f["key"]: (f.get("choices") or ["2569"])[0] for f in st["fields"] if f.get("required")}
c, _, url, _ = start(pub, f"/upload/{tok}/chunks", len(data), {"filename": f"link-{tag}.pdf", "submitter_name": "Somchai", "submitter_email": "s@example.com", "fields": json.dumps(req)})
c2, _, h = send(pub, url, data, 4 * MB)
_, lst = anong.j("GET", f"/files?system_id={SYS}&limit=50")
landed = [x for x in lst if x["name"] == f"link-{tag}.pdf"]
check("Upload link: name required before upload", c_noname == 400, f"{c_noname} {b_noname[:60]!r}")
check("Upload link: anonymous chunked upload lands in the basket", c == 201 and c2 == 204 and not h.get("Upload-File-Id") and len(landed) == 1,
      f"create {c} · done {c2} · in basket {len(landed)==1} · owner {landed[0]['owner'] if landed else '-'}")
c_steal, _, _ = anong.req("HEAD", url.replace("/upload/" + tok + "/chunks", "/uploads"), None, T)
check("Link session not reachable through the signed-in route", c_steal == 404, str(c_steal))

print(f"\n{sum(R)}/{len(R)} passed")
