"""Prepare the upload burst test: N editor accounts with sessions (default 400)."""
import json, os, sys, urllib.request, http.cookiejar, concurrent.futures as cf
B = os.environ.get("FH", "http://127.0.0.1:8090") + "/fh/api"
N = int(sys.argv[1]) if len(sys.argv) > 1 else 400
def opener():
    cj = http.cookiejar.CookieJar(); return cj, urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))
def post(op, p, body):
    try: return op.open(urllib.request.Request(B + p, data=json.dumps(body).encode(), headers={"content-type": "application/json"})).status
    except urllib.error.HTTPError as e: return e.code
_, adm = opener(); post(adm, "/auth/login", {"email": "admin@acme.go.th", "password": "admin123"})
def one(i):
    email = f"burst{i}@acme.go.th"
    post(adm, "/users", {"email": email, "display_name": f"Burst {i}", "password": "burst12345", "role": "editor"})
    cj, op = opener(); post(op, "/auth/login", {"email": email, "password": "burst12345"})
    return next((c.value for c in cj if c.name == "filehub_session"), None)
with cf.ThreadPoolExecutor(16) as ex: cookies = [c for c in ex.map(one, range(N)) if c]
json.dump({"cookies": cookies}, open(os.path.join(os.path.dirname(__file__), "ctx_upload.json"), "w"))
print(len(cookies), "sessions")
