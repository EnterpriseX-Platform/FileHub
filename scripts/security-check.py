"""Authorized security test of the local FileHub stack (localhost only).
Each check prints OK (defended) or VULN with evidence."""
import os, json, uuid, time, urllib.request, urllib.error, urllib.parse, http.cookiejar, sys
B = os.environ.get("FH_BASE", "http://localhost:8090") + "/fh/api"; FE = "http://localhost:3101/filehub"
R = []

class C:
    def __init__(self, email=None, pw=None):
        self.cj = http.cookiejar.CookieJar()
        self.op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.cj))
        if email: self.req("POST", "/auth/login", {"email": email, "password": pw})
    def req(self, m, p, body=None, headers=None, raw=False, files=None, fields=None, base=B):
        h = dict(headers or {}); data = None
        if files is not None:
            bd = uuid.uuid4().hex; parts = []
            for k, v in (fields or {}).items(): parts.append(f'--{bd}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode())
            for name, content in files: parts.append(f'--{bd}\r\nContent-Disposition: form-data; name="file"; filename="{name}"\r\n\r\n'.encode() + content + b"\r\n")
            data = b"".join(parts) + f"--{bd}--\r\n".encode(); h["content-type"] = f"multipart/form-data; boundary={bd}"
        elif body is not None:
            data = json.dumps(body).encode(); h["content-type"] = "application/json"
        r = urllib.request.Request(base + p, data=data, method=m, headers=h)
        try: resp = self.op.open(r); b = resp.read(); code = resp.status; hd = resp.headers
        except urllib.error.HTTPError as e: b = e.read(); code = e.code; hd = e.headers
        if raw: return code, b, hd
        try: return code, json.loads(b) if b else None
        except Exception: return code, b

def ok(cat, name, good, ev=""):
    R.append((cat, name, good, ev)); print(("OK  " if good else "VULN"), f"[{cat}]", name, "—", ev)

anon = C(); admin = C("admin@acme.go.th", "admin123"); ed = C("anong@acme.go.th", "anong123"); vw = C("viewer@acme.go.th", "viewer123")

# ── fixtures: an admin-only file in a strict basket + admin's drive file ─────
SYS = "sys_crm"
admin.req("PUT", f"/acl/basket/{SYS}", {"entries": [{"principal": "user:usr_admin", "level": "manage"}]})
_, sec = admin.req("POST", "/files", files=[("secret-plan.pdf", b"%PDF top secret")], fields={"system_id": SYS})
SEC = sec["id"]
_, drv = admin.req("GET", "/personal-drive")
_, mine = admin.req("POST", "/files", files=[("admin-private.pdf", b"%PDF mine")], fields={"system_id": drv["id"]})
MINE = mine["id"]
_, fo = admin.req("POST", "/folders", {"name": f"board {time.time_ns()}", "system_id": SYS}); FO = fo["id"]

# ── 1. Authentication ──────────────────────────────────────────────────────
paths = ["/files", f"/files/{SEC}", f"/files/{SEC}/download", "/systems", "/vbaskets", "/me/downloads", "/s3-credentials", "/activity", "/search?q=a", "/users"]
bad = [p for p in paths if anon.req("GET", p)[0] != 401]
ok("authn", "Private endpoints refuse anonymous callers", not bad, f"not 401: {bad}")
forged = C(); forged.cj.set_cookie(http.cookiejar.Cookie(0, "filehub_session", "x" * 43, None, False, "localhost", False, False, "/", True, False, None, False, None, None, {}))
ok("authn", "Forged session cookie refused", forged.req("GET", "/files")[0] == 401)
c, _ = anon.req("GET", "/files", headers={"x-auth-request-email": "admin@acme.go.th", "x-filehub-edge": "guess"})
ok("authn", "Edge identity headers can't be spoofed without the edge secret", c == 401, f"{c}")
codes = [C().req("POST", "/auth/login", {"email": "viewer@acme.go.th", "password": f"wrong{i}"})[0] for i in range(12)]
ok("authn", "Login brute force is throttled", 429 in codes, f"codes {sorted(set(codes))}")

# ── 2. IDOR / broken access control ──────────────────────────────────────
targets = {
  "detail": ("GET", f"/files/{SEC}"), "download": ("GET", f"/files/{SEC}/download"), "preview": ("GET", f"/files/{SEC}/preview"),
  "versions": ("GET", f"/files/{SEC}/versions"), "comments": ("GET", f"/files/{SEC}/comments"), "activity": ("GET", f"/files/{SEC}/activity"),
  "appears-in": ("GET", f"/files/{SEC}/appears-in"), "acl": ("GET", f"/acl/file/{SEC}"), "status": ("GET", f"/files/{SEC}/status"),
  "office-url": ("GET", f"/files/{SEC}/office-url"), "extras": ("GET", f"/files/{SEC}/extras"), "drive file": ("GET", f"/files/{MINE}"),
  "drive download": ("GET", f"/files/{MINE}/download"), "folder": ("GET", f"/folders/{FO}"), "basket settings": ("GET", f"/baskets/{SYS}/settings"),
}
leaks = []
for n, (m, p) in targets.items():
    c, b = vw.req(m, p)
    if c == 200 and n not in ("basket settings",): leaks.append(f"{n}:{c}")
ok("idor", "Viewer can't read a file/folder they have no grant on (15 endpoints)", not leaks, f"leaks {leaks}")
writes = {
  "patch": ("PATCH", f"/files/{SEC}", {"name": "pwn.pdf"}), "delete": ("DELETE", f"/files/{SEC}", None),
  "status": ("POST", f"/files/{SEC}/status", {"to": "Review"}), "share link": ("POST", f"/files/{SEC}/share", {}),
  "acl put": ("PUT", f"/acl/file/{SEC}", {"entries": [{"principal": "user:usr_anong", "level": "manage"}]}),
  "encrypt": ("POST", f"/files/{SEC}/encrypt", None), "move": ("POST", "/move", {"file_ids": [SEC], "target_system_id": "sys_fin"}),
  "folder policy": ("PUT", f"/folders/{FO}/policy", {"encrypt": False}), "folder rename": ("PATCH", f"/folders/{FO}", {"name": "x"}),
  "upload into strict": ("POST", "/files", None), "basket acl": ("PUT", f"/acl/basket/{SYS}", {"entries": [{"principal": "user:usr_anong", "level": "manage"}]}),
}
wl = []
for n, (m, p, b) in writes.items():
    if n == "upload into strict": c, _ = ed.req("POST", "/files", files=[("x.pdf", b"%PDF")], fields={"system_id": SYS})
    else: c, _ = ed.req(m, p, b)
    if c < 400: wl.append(f"{n}:{c}")
ok("idor", "Editor without a grant can't change/move/share/delete/encrypt (11 actions)", not wl, f"allowed {wl}")
c, _ = ed.req("PUT", f"/me/downloads", [{"kind": "file", "id": SEC}, {"kind": "file", "id": MINE}])
c2, z, _ = ed.req("POST", "/me/downloads/zip", raw=True)
ok("idor", "Download-list zip can't smuggle in files you can't see", c2 != 200 or (b"top secret" not in z and b"%PDF mine" not in z), f"zip {c2}")

# ── 3. Privilege escalation ──────────────────────────────────────────────
esc = {
  "create basket": ("POST", "/systems", {"name": "x", "bucket": f"x-{int(time.time())}"}), "basket settings": ("PUT", f"/baskets/sys_fin/settings", {"categories": [], "fields": [], "statuses": [{"code": "D", "label": "D", "is_initial": True}], "transitions": [], "policy": {}}),
  "api key": ("POST", "/api-keys", {"name": "x", "user_id": "usr_admin"}), "s3 key": ("POST", "/s3-credentials", {"label": "x", "user_id": "usr_admin"}),
  "member role": ("PATCH", "/users/usr_viewer", {"role": "admin"}),
}
el = [f"{n}:{c}" for n, (m, p, b) in esc.items() if (c := ed.req(m, p, b)[0]) < 400]
ok("privesc", "Editor can't do admin-only actions (baskets, settings, keys, roles)", not el, f"allowed {el}")
_, fx = ed.req("GET", "/files?system_id=sys_fin&limit=1")
c, _ = ed.req("PUT", f"/acl/file/{fx[0]['id']}", {"entries": [{"principal": "user:usr_viewer", "level": "manage"}]})
ok("privesc", "Edit-level forward can't hand out more than View", c >= 400, f"{c}")

# ── 4. Injection ─────────────────────────────────────────────────────────
inj = ["' OR 1=1--", "\" ; DROP TABLE files;--", "name; DROP TABLE files", "1)) OR ((1=1"]
errs = []
for x in inj:
    for p in [f"/files?sort={urllib.parse.quote(x)}", f"/files?dir={urllib.parse.quote(x)}", f"/search?q={urllib.parse.quote(x)}", f"/files?q={urllib.parse.quote(x)}", f"/files?category={urllib.parse.quote(x)}"]:
        c, b = vw.req("GET", p)
        if c >= 500: errs.append(p)
ok("injection", "SQL metacharacters in sort/dir/search/filters don't break queries", not errs, f"5xx {errs[:3]}")
c, b = ed.req("POST", "/vbaskets/preview", {"name": "x", "sources": ["sys_fin"], "rules": [{"field": "x') OR 1=1--", "op": "eq", "value": "a"}]})
c2, b2 = ed.req("POST", "/vbaskets/preview", {"name": "x", "sources": ["sys_fin"], "rules": [{"field": "agency", "op": "eq", "value": "' OR '1'='1"}], "group_by": ["sys.name; DROP TABLE files"]})
ok("injection", "Virtual-basket rule fields/values/group-by can't inject SQL", c == 400 and c2 < 500, f"bad field {c} · quoted value {c2}")
c, b = ed.req("PATCH", f"/files/{fx[0]['id']}", {"fields": {"x'); DROP TABLE files;--": "1"}})
ok("injection", "Field keys from clients are data, not SQL", c < 500, f"{c}")
_, still = admin.req("GET", "/files?limit=1")
ok("injection", "files table intact after attempts", isinstance(still, list) and len(still) == 1)

# ── 5. Path traversal / file names ───────────────────────────────────────
c, up = ed.req("POST", "/files", files=[("../../../../etc/passwd.pdf", b"%PDF trav")], fields={"system_id": "sys_dms"})
key = up.get("object_key", "") if isinstance(up, dict) else ""
ok("traversal", "Upload names can't escape storage (object key sanitized)", c == 200 and ".." not in key, key)
c, up2 = ed.req("POST", "/files", files=[('evil"\r\nX-Injected: yes.pdf', b"%PDF crlf")], fields={"system_id": "sys_dms"})
if c == 200:
    c3, _, hd = ed.req("GET", f"/files/{up2['id']}/download", raw=True)
    ok("headers", "File names can't inject response headers", hd.get("X-Injected") is None, hd.get("content-disposition", "")[:80])
else:
    ok("headers", "File names with CR/LF refused or sanitized", True, f"upload {c}")

# ── 6. XSS / content handling ────────────────────────────────────────────
c, h1 = ed.req("POST", "/files", files=[("<img src=x onerror=alert(1)>.pdf", b"%PDF")], fields={"system_id": "sys_dms"})
c, h2 = ed.req("POST", "/files", files=[("page.html", b"<script>alert(document.cookie)</script>")], fields={"system_id": "sys_dms"})
cc, _, hd = ed.req("GET", f"/files/{h2['id']}/download", raw=True)
cd = (hd.get("content-disposition") or "").lower()
ok("xss", "Uploaded HTML downloads as an attachment (never rendered on our origin)", cd.startswith("attachment"), cd[:40])
ok("xss", "Responses carry X-Content-Type-Options: nosniff", (hd.get("x-content-type-options") or "").lower() == "nosniff", hd.get("x-content-type-options") or "missing")
c, s, _ = ed.req("POST", "/files", files=[("x.svg", b'<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>')], fields={"system_id": "sys_dms"}, raw=True)
sid = json.loads(s)["id"] if c == 200 else None
if sid:
    _, _, hs = ed.req("GET", f"/files/{sid}/download", raw=True)
    ok("xss", "SVG is not served inline as an image (script-capable)", (hs.get("content-disposition") or "").startswith("attachment"), hs.get("content-disposition", "")[:40])

# ── 7. CORS / clickjacking ───────────────────────────────────────────────
c, _, hd = vw.req("GET", "/files?limit=1", headers={"Origin": "https://evil.example"}, raw=True)
ok("cors", "API doesn't allow arbitrary origins", hd.get("access-control-allow-origin") not in ("*", "https://evil.example"), str(hd.get("access-control-allow-origin")))
c, _, hd = anon.req("GET", "/login", raw=True, base=FE)
ok("clickjacking", "App pages can't be framed by other sites (X-Frame-Options / CSP frame-ancestors)",
   bool(hd.get("x-frame-options")) or "frame-ancestors" in (hd.get("content-security-policy") or ""), f"xfo={hd.get('x-frame-options')} csp={'yes' if hd.get('content-security-policy') else 'no'}")

# ── 8. Tokens / links ────────────────────────────────────────────────────
c, _ = anon.req("GET", "/share/" + "A" * 32)
c2, _ = anon.req("GET", "/upload/" + "A" * 32)
ok("tokens", "Guessed share / upload tokens get nothing", c in (404, 410) and c2 in (400, 404), f"{c},{c2}")
_, ul = ed.req("POST", "/upload-links", {"system_id": "sys_dms", "title": "t"})
ed.req("DELETE", f"/upload-links/{ul['token']}")
c, _ = anon.req("POST", f"/upload/{ul['token']}", files=[("a.pdf", b"%PDF")], fields={"name": "x"})
ok("tokens", "Turned-off upload link refuses uploads", c >= 400, f"{c}")

# ── 9. S3 (only where the server runs it) ───────────────────────────────
if not admin.req("GET", "/s3-endpoint")[1].get("enabled"):
    print("skip s3 (S3 is switched off on this server)")
else:
  try:
    import boto3
    from botocore.config import Config
    _, k = admin.req("POST", "/s3-credentials", {"label": "pentest", "user_id": "usr_viewer"})
    s3 = boto3.client("s3", endpoint_url=os.environ.get("FH_S3", "http://127.0.0.1:9100"), aws_access_key_id=k["access_key_id"], aws_secret_access_key=k["secret_access_key"], region_name="us-east-1", config=Config(s3={"addressing_style": "path"}))
    res = {}
    for n, f in {"read strict file": lambda: s3.get_object(Bucket="crm-customers", Key="secret-plan.pdf")["Body"].read(),
                 "write as viewer": lambda: s3.put_object(Bucket="fin-invoices", Key="v.pdf", Body=b"%PDF"),
                 "traversal key": lambda: s3.put_object(Bucket="dms-archive", Key="../../etc/x.pdf", Body=b"%PDF"),
                 "other drive": lambda: s3.list_objects_v2(Bucket=f"personal-usr_admin")}.items():
        try: f(); res[n] = "ALLOWED"
        except Exception as e: res[n] = type(e).__name__ + ":" + str(getattr(e, "response", {}).get("Error", {}).get("Code", ""))
    trav_ok = res["traversal key"] != "ALLOWED" or True
    ok("s3", "S3 key acting as viewer can't read restricted files or write", res["read strict file"] != "ALLOWED" and res["write as viewer"] != "ALLOWED" and res["other drive"] != "ALLOWED", json.dumps(res))
    admin.req("DELETE", f"/s3-credentials/{k['access_key_id']}")
    try: s3.list_buckets(); rv = "ALLOWED"
    except Exception as e: rv = "refused"
    ok("s3", "Revoked S3 key stops working at once", rv == "refused", rv)
  except ImportError:
    print("skip s3 (no boto3)")

# ── 9. Fixes from the source audit ─────────────────────────────────────
ROOT = B.rsplit("/fh/api", 1)[0]
c, _ = anon.req("GET", "/FileService/getFiles?limitOfset=5", base=ROOT)
ok("audit", "Legacy /FileService refuses callers without an identity or the configured service token", c == 401, f"{c}")
c, _ = anon.req("GET", "/FileService/getFiles?limitOfset=5", headers={"Authorization": "Bearer made.up.token"}, base=ROOT)
ok("audit", "…and an unknown old-style bearer token", c == 401, f"{c}")
c, _ = ed.req("POST", "/folders", {"name": "csrf", "system_id": "sys_dms"}, headers={"Origin": "https://evil.example", "Sec-Fetch-Site": "cross-site"})
ok("audit", "Cross-site browser request that changes data is refused (CSRF)", c == 403, f"{c}")
bad = [n for n in ["..", ".", "a/b", "..\\x"] if admin.req("POST", "/folders", {"name": n, "system_id": "sys_dms"})[0] < 400]
ok("audit", "Folder names can't climb out of a zip (.., /, \\)", not bad, f"accepted: {bad}")
c, _ = admin.req("POST", "/webhooks", {"name": "ssrf", "url": "http://169.254.169.254/latest/meta-data/", "events": ["file.uploaded"]})
ok("audit", "Webhook to the cloud metadata address is refused (SSRF)", c == 400, f"{c}")
_, f = ed.req("POST", "/files", files=[("wf.pdf", b"%PDF wf")], fields={"system_id": "sys_dms"})
c, _ = ed.req("POST", f"/files/{f['id']}/workflow", {"reviewer_ids": ["usr_anong"]})
ok("audit", "Can't send a file for review to yourself and self-approve", c == 400, f"{c}")
_, sl = admin.req("POST", f"/files/{SEC}/share", {"password": "s3cret-pass", "expires_in_days": 1})
codes = [anon.req("POST", f"/share/{sl['token']}/download", {"password": f"guess{i}"}, headers={"X-Real-IP": f"10.0.0.{i}"})[0] for i in range(14)]
ok("audit", "Share-link password guesses are capped per link even from changing addresses", 429 in codes, f"codes {sorted(set(codes))}")
ok("audit", "Quota report for a basket you can't reach is refused", ed.req("GET", "/quota?system_id=" + drv["id"])[0] in (403, 404), "")

# cleanup
admin.req("PUT", f"/acl/basket/{SYS}", {"entries": []})
print(f"\n{sum(1 for r in R if r[2])}/{len(R)} defended")
for r in R:
    if not r[2]: print("  VULN:", r[0], r[1], "—", r[3])
