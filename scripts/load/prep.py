"""Prepare a load test: 20 editor users with sessions, small files to download,
a virtual basket over the 50k rows, presigned S3 URLs."""
import json, os, urllib.request, http.cookiejar, uuid
B = "http://127.0.0.1:8090/fh/api"
here = os.path.dirname(__file__)

def opener():
    cj = http.cookiejar.CookieJar(); return cj, urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))
def call(op, m, p, body=None, files=None, fields=None):
    h = {}; data = None
    if files:
        bd = uuid.uuid4().hex; parts = [f'--{bd}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode() for k, v in (fields or {}).items()]
        for n, c in files: parts.append(f'--{bd}\r\nContent-Disposition: form-data; name="file"; filename="{n}"\r\n\r\n'.encode() + c + b"\r\n")
        data = b"".join(parts) + f"--{bd}--\r\n".encode(); h["content-type"] = f"multipart/form-data; boundary={bd}"
    elif body is not None: data = json.dumps(body).encode(); h["content-type"] = "application/json"
    try:
        r = op.open(urllib.request.Request(B + p, data=data, method=m, headers=h)); return r.status, json.loads(r.read() or b"null")
    except urllib.error.HTTPError as e: return e.code, e.read()

_, adm = opener(); call(adm, "POST", "/auth/login", {"email": "admin@acme.go.th", "password": "admin123"})
cookies = []
for i in range(20):
    email = f"load{i}@acme.go.th"
    call(adm, "POST", "/users", {"email": email, "display_name": f"Load {i}", "password": "loadtest123", "role": "editor"})
    cj, op = opener(); c, _ = call(op, "POST", "/auth/login", {"email": email, "password": "loadtest123"})
    cookies += [ck.value for ck in cj if ck.name == "filehub_session"]
files = []
for i in range(20):
    c, f = call(adm, "POST", "/files", files=[(f"dl-{i}.pdf", b"%PDF-1.4 " + os.urandom(48 * 1024))], fields={"system_id": "sys_dms"})
    files.append(f["id"])
c, v = call(adm, "POST", "/vbaskets", {"name": "Load test", "sources": ["sys_fin", "sys_dms", "sys_hr"], "rules": [{"field": "sys.format", "op": "in", "value": ["PDF", "Word"]}], "group_by": ["agency", "fiscal_year"]})
open(os.path.join(here, "small.pdf"), "wb").write(b"%PDF-1.4 " + os.urandom(20 * 1024))
presigned = []
try:
    import boto3
    from botocore.config import Config
    c, k = call(adm, "POST", "/s3-credentials", {"label": "load", "user_id": "usr_admin"})
    s3 = boto3.client("s3", endpoint_url="http://127.0.0.1:9100", aws_access_key_id=k["access_key_id"], aws_secret_access_key=k["secret_access_key"], region_name="us-east-1", config=Config(s3={"addressing_style": "path"}))
    for i in range(10):
        s3.put_object(Bucket="dms-archive", Key=f"load/s3-{i}.pdf", Body=b"%PDF-1.4 " + os.urandom(32 * 1024))
        presigned.append(s3.generate_presigned_url("get_object", Params={"Bucket": "dms-archive", "Key": f"load/s3-{i}.pdf"}, ExpiresIn=3600))
except ImportError:
    pass
json.dump({"cookies": cookies, "files": files, "vbasket": v["id"], "presigned": presigned}, open(os.path.join(here, "ctx.json"), "w"))
print(len(cookies), "sessions,", len(files), "files,", len(presigned), "presigned, vbasket", v["id"])
