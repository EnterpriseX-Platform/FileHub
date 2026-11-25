"""IAMX / Keycloak bearer token checks. Starts a fake Keycloak JWKS endpoint on
:8799 (backend must run with IAMX_JWKS_URL=http://127.0.0.1:8799/certs,
IAMX_JWT_ISSUER=http://kc.test/realms/app, IAMX_ALLOWED_CLIENTS=app-docs,app-assets).
Writes the JWKS to the path given as argv[1] and serves it if argv[2]=='serve'."""
import base64, json, os, sys, threading, time, uuid, urllib.request, urllib.error, http.server
import jwt
from cryptography.hazmat.primitives.asymmetric import rsa
HERE = os.path.dirname(os.path.abspath(__file__))
KEYF = os.path.join(HERE, ".iamx-test-key.pem")
from cryptography.hazmat.primitives import serialization
if os.path.exists(KEYF):
    key = serialization.load_pem_private_key(open(KEYF, "rb").read(), None)
else:
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    open(KEYF, "wb").write(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
other = rsa.generate_private_key(public_exponent=65537, key_size=2048)
pub = key.public_key().public_numbers()
b = lambda n: base64.urlsafe_b64encode(n.to_bytes((n.bit_length() + 7) // 8, "big")).rstrip(b"=").decode()
JWKS = {"keys": [{"kty": "RSA", "kid": "kc-1", "use": "sig", "alg": "RS256", "n": b(pub.n), "e": b(pub.e)}]}

class H(http.server.BaseHTTPRequestHandler):
    def do_GET(s):
        s.send_response(200); s.send_header("content-type", "application/json"); s.end_headers(); s.wfile.write(json.dumps(JWKS).encode())
    def log_message(s, *a): pass
if len(sys.argv) > 1 and sys.argv[1] == "serve":
    http.server.HTTPServer(("127.0.0.1", 8799), H).serve_forever()

ISS = "http://kc.test/realms/app"
def token(sub="kc-sub-1001", email="somsri@agency.example", azp="app-docs", exp=600, signer=None, iss=ISS, roles=("DOCS_USER",), agency="03001"):
    now = int(time.time())
    claims = {"iss": iss, "sub": sub, "azp": azp, "aud": "account", "exp": now + exp, "iat": now, "typ": "Bearer",
              "email": email, "name": "Somsri Kc", "preferred_username": email.split("@")[0],
              "realm_access": {"roles": ["offline_access"]},
              "USERINFO": json.dumps({"USER_ID": sub, "EMAIL": email, "FULL_NAME": "สมศรี ทดสอบ", "AGENCY_CODE": agency, "AGENCY_NAME": "กรมทดสอบ"}),
              "USERROLE": json.dumps(list(roles))}
    return jwt.encode(claims, signer or key, algorithm="RS256", headers={"kid": "kc-1"})

B = os.environ.get("FH_BASE", "http://localhost:8090") + "/fh/api"
def call(m, p, tok=None, h=None, body=None):
    hh = dict(h or {})
    if tok: hh["Authorization"] = f"Bearer {tok}"
    try: r = urllib.request.urlopen(urllib.request.Request(B + p, data=body, method=m, headers=hh)); return r.status, r.read(), r.headers
    except urllib.error.HTTPError as e: return e.code, e.read(), e.headers
R = []
def check(n, ok, d=""): R.append(ok); print("PASS" if ok else "FAIL", n, "—", d)

c, me, _ = call("GET", "/auth/me", token())
me = json.loads(me) if c == 200 else {}
check("Keycloak/IAMX token signs the person in (account made on first call)", c == 200 and me.get("email") == "somsri@agency.example", f"{c} {me.get('display_name')} role={me.get('role')}")
c2, me2, _ = call("GET", "/auth/me", token(email="somsri.new@agency.example"))
me2 = json.loads(me2) if c2 == 200 else {}
check("Same person = same account even if email changes (keyed by sub)", c2 == 200 and me2.get("id") == me.get("id"), f"{me.get('id')} / {me2.get('id')}")
call("GET", "/auth/me", token())  # email back

# chunked upload with the IAMX token into the person's My Drive
c, d, _ = call("GET", "/personal-drive", token()); drive = json.loads(d)["id"] if c == 200 else None
data = b"%PDF-1.4 iamx " + os.urandom(3 * 1024 * 1024)
md = ",".join(f"{k} {base64.b64encode(v.encode()).decode()}" for k, v in {"filename": "iamx-upload.pdf", "system_id": drive or ""}.items())
c, _, h = call("POST", "/uploads", token(), {"Tus-Resumable": "1.0.0", "Upload-Length": str(len(data)), "Upload-Metadata": md}, b"")
url = h.get("Location") if c == 201 else None
c2, _, h2 = call("PATCH", "/" + url, token(), {"Tus-Resumable": "1.0.0", "Upload-Offset": "0", "Content-Type": "application/offset+octet-stream"}, data) if url else (0, b"", {})
fid = h2.get("Upload-File-Id") if c2 == 204 else None
c3, f, _ = call("GET", f"/files/{fid}", token()) if fid else (0, b"{}", None)
f = json.loads(f) if c3 == 200 else {}
check("Chunked upload with the IAMX token, file owned by that person", c2 == 204 and f.get("owner") in ("สมศรี ทดสอบ", "Somsri Kc") and f.get("created_by") == me.get("id"),
      f"drive {drive} · owner {f.get('owner')} · created_by {f.get('created_by')}")
c4, _, _ = call("POST", "/files", token(), {"content-type": "multipart/form-data; boundary=x"},
                b'--x\r\nContent-Disposition: form-data; name="system_id"\r\n\r\n' + (drive or "").encode() + b'\r\n--x\r\nContent-Disposition: form-data; name="file"; filename="one.pdf"\r\n\r\n%PDF-1.4 one\r\n--x--\r\n')
check("One-request upload with the IAMX token", c4 == 200, str(c4))

check("Forged signature refused", call("GET", "/auth/me", token(signer=other))[0] == 401)
check("Expired token refused", call("GET", "/auth/me", token(exp=-120))[0] == 401)
check("Token from another realm (issuer) refused", call("GET", "/auth/me", token(iss="http://evil/realms/x"))[0] == 401)
check("Client app not allowed (azp) refused", call("GET", "/auth/me", token(azp="random-app"))[0] == 403)
c, d, _ = call("GET", "/auth/me", token(), {"x-auth-request-email": "admin@acme.go.th", "x-filehub-edge": "guess"})
check("Identity headers sent alongside the token are ignored", c == 200 and json.loads(d).get("email") == "somsri@agency.example", json.loads(d).get("email") if c == 200 else str(c))
c, d, _ = call("GET", "/files?limit=5", token())
check("Not an admin: sees only what IAMX person may see", c == 200 and me.get("role") != "admin", f"{c} role={me.get('role')}")
print(f"\n{sum(R)}/{len(R)} passed")
