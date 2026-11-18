"""Legacy /FileService/* compatibility checks, written from how real callers use it:
a browser Dropzone plugin, a signing back-end (Go), an HR back-end (Java) and a
budget app's REST client (getFiles / previewFile / rename / moveFileToTrash).
Sets up its own connectors (Admin → Connectors). Backend needs EDGE_AUTH_SECRET=edgetest."""
import base64, json, os, time, uuid, urllib.request, urllib.error, urllib.parse, http.cookiejar
ROOT = os.environ.get("FH_BASE", "http://localhost:8090")
R = []
def check(n, ok, d=""): R.append(bool(ok)); print("PASS" if ok else "FAIL", n, "—", d)

def b64(d): return base64.urlsafe_b64encode(json.dumps(d).encode()).rstrip(b"=").decode()
def edge(email, name, roles=("APP_USER",), sub=None):
    tok = b64({"alg": "none"}) + "." + b64({"sub": sub or "edge-" + email, "USERINFO": json.dumps({"EMAIL": email, "FULL_NAME": name, "USER_ID": sub or email}), "USERROLE": json.dumps(list(roles))}) + ".x"
    return {"x-filehub-edge": "edgetest", "x-forwarded-access-token": tok}
CENTER = {"Authorization": "Bearer eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiJzeXN0ZW1fYXBwIn0.not-verifiable"}

def req(m, p, h=None, body=None):
    try: r = urllib.request.urlopen(urllib.request.Request(ROOT + p, data=body, method=m, headers=h or {})); return r.status, r.read(), r.headers
    except urllib.error.HTTPError as e: return e.code, e.read(), e.headers
def form(fields, files):
    bd = uuid.uuid4().hex; parts = []
    for k, v in fields:
        if k == "__file__":
            n, c = v; parts.append(f'--{bd}\r\nContent-Disposition: form-data; name="file"; filename="{n}"\r\nContent-Type: application/pdf\r\n\r\n'.encode() + c + b"\r\n")
        else: parts.append(f'--{bd}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode())
    return b"".join(parts) + f"--{bd}--\r\n".encode(), {"content-type": f"multipart/form-data; boundary={bd}"}
J = lambda b: json.loads(b)
tag = time.strftime("%H%M%S")
somchai = edge(f"somchai.{tag}@agency.example", "สมชาย ใจดี")

# Connectors for this run: a path prefix → sys_crm, the old service token → sys_dms, and a read-only one.
_ac = http.cookiejar.CookieJar(); _ao = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(_ac))
_ao.open(urllib.request.Request(ROOT + "/fh/api/auth/login", data=json.dumps({"email": "admin@acme.go.th", "password": "admin123"}).encode(), headers={"content-type": "application/json"}))
def admin(m, p, body=None):
    try: r = _ao.open(urllib.request.Request(ROOT + "/fh/api" + p, data=json.dumps(body).encode() if body is not None else None, method=m, headers={"content-type": "application/json"})); b = r.read(); return r.status, json.loads(b) if b else None
    except urllib.error.HTTPError as e: return e.code, e.read()
for con in admin("GET", "/admin/connectors")[1]:
    if con["name"].startswith("test:"): admin("DELETE", f"/admin/connectors/{con['id']}")
READONLY = {"Authorization": "Bearer readonly-token-" + "x" * 16}
admin("POST", "/admin/connectors", {"name": "test: forms app", "path_prefixes": ["forms-app"], "system_id": "sys_crm"})
admin("POST", "/admin/connectors", {"name": "test: signing service", "token": CENTER["Authorization"][7:], "system_id": "sys_dms"})
admin("POST", "/admin/connectors", {"name": "test: read-only", "token": READONLY["Authorization"][7:], "system_id": "sys_dms", "read_only": True})

# 1. Browser Dropzone plugin: fileName, fileType, groupId, file, parentFolderId (folder AFTER the file) + center Bearer + edge SSO
folder = f"5de3e1ca-legacy-{tag}"
pdf = b"%PDF-1.4 app " + os.urandom(2000)
body, h = form([("fileName", f"รายงาน-{tag}.pdf"), ("fileType", "application/pdf"), ("groupId", ""), ("__file__", (f"รายงาน-{tag}.pdf", pdf)), ("parentFolderId", folder)], None)
c, b, _ = req("POST", "/FileService/upload", {**h, **CENTER, **somchai, "Referer": "https://app.example.org/forms-app/page/x"}, body)
r = J(b) if c == 200 else {}
d = r.get("data", {})
fid = d.get("id")
check("Plugin upload: same response keys (data.id, file_name w/o ext, mime_type, file_path)", c == 200 and fid and d.get("file_name") == f"รายงาน-{tag}" and d.get("mime_type") == "application/pdf" and "file_path" in d, f"{c} {str(b)[:90] if c != 200 else ''}")
_, me, _ = req("GET", "/fh/api/auth/me", somchai)
check("Plugin upload: recorded as the signed-in person, not the service account", d.get("user_id") == J(me).get("id"), f"user_id {d.get('user_id')} · person {J(me).get('id')}")
check("Plugin upload: folder kept by its old id, basket from the app's path (Referer map)", d.get("parent_folder_id") == folder, f"folder {d.get('parent_folder_id')}")
_, fl, _ = req("GET", f"/fh/api/files/{fid}", somchai)
check("…landed in sys_crm via the forms-app connector prefix", J(fl).get("system_id") == "sys_crm", J(fl).get("system_id"))
body, h = form([("__file__", ("x.pdf", b"%PDF-1.4 x"))], None)
_closed = json.loads(_ao.open(urllib.request.Request(ROOT + "/fh/api/systems", data=json.dumps({"name": f"Closed {tag}", "bucket": f"closed-{tag}", "tone": "slate"}).encode(), headers={"content-type": "application/json"})).read())["id"]
_ao.open(urllib.request.Request(ROOT + f"/fh/api/acl/basket/{_closed}", data=json.dumps({"entries": [{"principal": "user:usr_admin", "level": "manage"}]}).encode(), method="PUT", headers={"content-type": "application/json"}))
c, b, _ = req("POST", "/FileService/upload", {**h, **CENTER, **somchai, "x-filehub-system": _closed}, body)
check("Upload into a basket the person has no access to is refused", c == 403, f"{c} {b[:50]!r}")

# 2. Signing back-end (no edge): Bearer center token only → service account; reads data.file_size
signed = b"%PDF-1.4 signed " + os.urandom(5000)
body, h = form([("__file__", ("signed.pdf", signed)), ("parentFolderId", "")], None)
c, b, _ = req("POST", "/FileService/upload", {**h, **CENTER}, body)
d2 = J(b).get("data", {}) if c == 200 else {}
check("Back-end upload (no SSO): accepted as its connector's account, file_size given", c == 200 and d2.get("file_size") == len(signed) and str(d2.get("user_id", "")).startswith("usr_con_"), f"{c} size {d2.get('file_size')} user {d2.get('user_id')}")
c, _, _ = req("POST", "/FileService/upload", {**h, **READONLY}, body)
c2, _, _ = req("GET", "/FileService/getFiles?offset=0&limitOfset=5", READONLY)
check("Read-only connector may list but not upload", c == 403 and c2 == 200, f"upload {c} · list {c2}")

# 3. HR back-end uploadFile: parent_folder_id + reads data.file_app_id
body, h = form([("__file__", ("hr.pdf", b"%PDF-1.4 hr")), ("parent_folder_id", folder)], None)
c, b, _ = req("POST", "/FileService/uploadFile", {**h, **CENTER, **somchai}, body)
check("HR back-end uploadFile: data.file_app_id", c == 200 and J(b)["data"].get("file_app_id"), str(c))

# 4. downloadFile as the plugin calls it: plain fetch, same origin (edge SSO only, no Authorization)
c, b, hh = req("GET", f"/FileService/downloadFile?fileId={fid}&mimetype=application/octet-stream", somchai)
cd = hh.get("Content-Disposition", "") if hh else ""
check("downloadFile: bytes + Thai file name in filename*=UTF-8''", c == 200 and b == pdf and "filename*=UTF-8''" in cd, f"{c} {cd[:70]}")

# 5. REST client getFiles: data[] + paging.total, sub-folders as DIRECTORY
_, fo, _ = req("POST", "/fh/api/folders", {**somchai, "content-type": "application/json"}, json.dumps({"system_id": "sys_crm", "parent_id": folder, "name": f"sub-{tag}"}).encode())
c, b, _ = req("GET", f"/FileService/getFiles?offset=0&limitOfset=30&searchDir=Y&parentFolderId={folder}", {**CENTER, **somchai})
g = J(b) if c == 200 else {}
kinds = sorted({x.get("type_") for x in g.get("data", [])})
check("getFiles: files + sub-folders, paging.total", c == 200 and g.get("paging", {}).get("total") == 2 and kinds == ["DIRECTORY", "FILE"], f"total {g.get('paging')} kinds {kinds} · subfolder create {_}")

# 6. previewFile inline
c, b, hh = req("GET", f"/FileService/previewFile?fileId={fid}&logType=preview", somchai)
check("previewFile: inline", c == 200 and hh.get("Content-Disposition", "").startswith("inline"), f"{c} {hh.get('Content-Disposition','')[:30] if hh else ''}")

# 7. rename without the extension keeps it
c, b, _ = req("POST", "/FileService/rename", {**somchai, "content-type": "application/json"}, json.dumps({"fileId": fid, "renameTo": f"ชื่อใหม่-{tag}"}).encode())
_, fl, _ = req("GET", f"/fh/api/files/{fid}", somchai)
check("rename: keeps the extension", c == 200 and J(fl).get("name") == f"ชื่อใหม่-{tag}.pdf", J(fl).get("name"))

# 8. security: another person with view-only can't trash; a stranger can't read a file in a strict basket
viewer = edge(f"viewer.{tag}@agency.example", "Viewer", roles=())
adminc = http.cookiejar.CookieJar(); op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(adminc))
op.open(urllib.request.Request(ROOT + "/fh/api/auth/login", data=json.dumps({"email": "admin@acme.go.th", "password": "admin123"}).encode(), headers={"content-type": "application/json"}))
_, vme, _ = req("GET", "/fh/api/auth/me", viewer); vid = J(vme)["id"]
op.open(urllib.request.Request(ROOT + f"/fh/api/users/{vid}", data=json.dumps({"role": "viewer"}).encode(), method="PATCH", headers={"content-type": "application/json"}))
c, b, _ = req("POST", "/FileService/moveFileToTrash", {**viewer, "content-type": "application/json"}, json.dumps({"fileId": fid, "type": "FILE"}).encode())
check("moveFileToTrash: a viewer is refused", c in (403, 404), str(c))
op.open(urllib.request.Request(ROOT + f"/fh/api/acl/folder/{folder}", data=json.dumps({"entries": [{"principal": f"user:{J(me)['id']}", "level": "manage"}], "inherit": False}).encode(), method="PUT", headers={"content-type": "application/json"}))
c1, _, _ = req("GET", f"/FileService/downloadFile?fileId={fid}", viewer)
c2, b2, _ = req("GET", f"/FileService/getFiles?parentFolderId={folder}", viewer)
check("Folder closed to one person: others get 404 on download and see nothing in getFiles", c1 == 404 and J(b2).get("total") == 0, f"download {c1} · getFiles total {J(b2).get('total')}")
c3, _, _ = req("GET", f"/FileService/downloadFile?fileId={fid}", somchai)
check("…the person with access still downloads", c3 == 200, str(c3))

# 9. trash file and folder (the REST client sends type)
c, b, _ = req("POST", "/FileService/moveFileToTrash", {**somchai, "content-type": "application/json"}, json.dumps({"fileId": fid, "type": "FILE"}).encode())
c2, _, _ = req("GET", f"/FileService/downloadFile?fileId={fid}", somchai)
sub_id = J(fo).get("id") if fo else None
c3, b3, _ = req("POST", "/FileService/moveFileToTrash", {**somchai, "content-type": "application/json"}, json.dumps({"fileId": sub_id, "type": "DIRECTORY"}).encode())
check("moveFileToTrash: FILE then DIRECTORY", c == 200 and c2 == 404 and c3 == 200, f"file {c} → download {c2} · folder {c3} {b3[:60]!r}")

# 10. unknown id with no old service configured → 404 (not 500)
c, _, _ = req("GET", "/FileService/downloadFile?fileId=does-not-exist", somchai)
check("Unknown old id without LEGACY_FILEHUB_URL → 404", c == 404, str(c))
print(f"\n{sum(R)}/{len(R)} passed")
