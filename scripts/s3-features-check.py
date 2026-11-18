"""MinIO-style S3 features, checked with the real AWS SDK (boto3).

  FH=http://127.0.0.1:8090 S3=http://127.0.0.1:9100 python3.11 scripts/s3-features-check.py

Versioning, ListParts / ListMultipartUploads, tagging, object lock (retention +
legal hold), large CopyObject, webhooks (outbox) and the bucket lock config.
"""
import datetime as dt, json, os, time, urllib.request, http.cookiejar, threading, http.server, hmac, hashlib
import boto3
from botocore.config import Config
from botocore.exceptions import ClientError

FH = os.environ.get("FH", os.environ.get("FH_BASE", "http://127.0.0.1:8090")) + "/fh/api"
S3 = os.environ.get("S3", os.environ.get("FH_S3", "http://127.0.0.1:9100"))
R = []
def check(n, ok, d=""): R.append(bool(ok)); print("PASS" if ok else "FAIL", n, "—", d)

cj = http.cookiejar.CookieJar(); op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))
def api(m, p, body=None):
    req = urllib.request.Request(FH + p, data=json.dumps(body).encode() if body is not None else None, method=m, headers={"content-type": "application/json"})
    try: r = op.open(req); b = r.read(); return r.status, (json.loads(b) if b else None)
    except urllib.error.HTTPError as e: return e.code, e.read()[:300]
api("POST", "/auth/login", {"email": "admin@acme.go.th", "password": "admin123"})
tag = time.strftime("%H%M%S")
_, b = api("POST", "/systems", {"name": f"S3 features {tag}", "bucket": f"s3f-{tag}", "tone": "slate"}); SYS, BUCKET = b["id"], f"s3f-{tag}"
_, k = api("POST", "/s3-credentials", {"label": f"s3f-{tag}", "user_id": "usr_admin"})
s3 = boto3.client("s3", endpoint_url=S3, aws_access_key_id=k["access_key_id"], aws_secret_access_key=k["secret_access_key"],
                  region_name="us-east-1", config=Config(s3={"addressing_style": "path"}, retries={"max_attempts": 1}))

# ── versioning
r1 = s3.put_object(Bucket=BUCKET, Key="docs/plan.txt", Body=b"version one")
r2 = s3.put_object(Bucket=BUCKET, Key="docs/plan.txt", Body=b"version two, longer")
check("PutObject returns a version id, and it moves on", r1.get("VersionId") == "1" and r2.get("VersionId") == "2", f"{r1.get('VersionId')} → {r2.get('VersionId')}")
old = s3.get_object(Bucket=BUCKET, Key="docs/plan.txt", VersionId="1")["Body"].read()
cur = s3.get_object(Bucket=BUCKET, Key="docs/plan.txt")["Body"].read()
check("GetObject ?versionId reads the old version; plain GET the newest", old == b"version one" and cur == b"version two, longer")
vs = s3.list_object_versions(Bucket=BUCKET, Prefix="docs/")["Versions"]
check("ListObjectVersions lists both, newest marked latest", sorted((v["VersionId"], v["IsLatest"]) for v in vs) == [("1", False), ("2", True)], str([(v["VersionId"], v["IsLatest"]) for v in vs]))
try: s3.delete_object(Bucket=BUCKET, Key="docs/plan.txt", VersionId="1"); ok = False
except ClientError as e: ok = e.response["Error"]["Code"] == "AccessDenied"
check("Deleting one past version is refused (history is kept)", ok)
check("GetBucketVersioning = Enabled", s3.get_bucket_versioning(Bucket=BUCKET).get("Status") == "Enabled")

# ── multipart: ListParts / ListMultipartUploads
mpu = s3.create_multipart_upload(Bucket=BUCKET, Key="big/video.bin")
part = os.urandom(5 * 1024 * 1024)
e1 = s3.upload_part(Bucket=BUCKET, Key="big/video.bin", UploadId=mpu["UploadId"], PartNumber=1, Body=part)["ETag"]
ups = s3.list_multipart_uploads(Bucket=BUCKET).get("Uploads", [])
parts = s3.list_parts(Bucket=BUCKET, Key="big/video.bin", UploadId=mpu["UploadId"]).get("Parts", [])
check("ListMultipartUploads shows the unfinished upload", any(u["UploadId"] == mpu["UploadId"] for u in ups))
check("ListParts shows the part already sent (resume)", [(p["PartNumber"], p["Size"]) for p in parts] == [(1, len(part))])
e2 = s3.upload_part(Bucket=BUCKET, Key="big/video.bin", UploadId=mpu["UploadId"], PartNumber=2, Body=b"tail")["ETag"]
s3.complete_multipart_upload(Bucket=BUCKET, Key="big/video.bin", UploadId=mpu["UploadId"], MultipartUpload={"Parts": [{"PartNumber": 1, "ETag": e1}, {"PartNumber": 2, "ETag": e2}]})
check("…and completes", s3.head_object(Bucket=BUCKET, Key="big/video.bin")["ContentLength"] == len(part) + 4)

# ── large CopyObject (streams, no memory spike)
s3.copy_object(Bucket=BUCKET, Key="big/video-copy.bin", CopySource={"Bucket": BUCKET, "Key": "big/video.bin"})
check("CopyObject of a 5 MB object", s3.head_object(Bucket=BUCKET, Key="big/video-copy.bin")["ContentLength"] == len(part) + 4)

# ── tagging
s3.put_object_tagging(Bucket=BUCKET, Key="docs/plan.txt", Tagging={"TagSet": [{"Key": "project", "Value": "plan-2570"}, {"Key": "owner", "Value": "docs"}]})
ts = {t["Key"]: t["Value"] for t in s3.get_object_tagging(Bucket=BUCKET, Key="docs/plan.txt")["TagSet"]}
check("Put/GetObjectTagging round trip", ts.get("project") == "plan-2570" and ts.get("owner") == "docs", str(ts))
_, found = api("GET", "/search?q=plan-2570")
check("…tags are searchable in FileHub", "plan.txt" in json.dumps(found))
s3.delete_object_tagging(Bucket=BUCKET, Key="docs/plan.txt")
check("DeleteObjectTagging clears them", s3.get_object_tagging(Bucket=BUCKET, Key="docs/plan.txt")["TagSet"] == [])

# ── object lock: retention + legal hold
s3.put_object(Bucket=BUCKET, Key="records/contract.pdf", Body=b"%PDF-1.4 contract")
until = (dt.datetime.now(dt.timezone.utc) + dt.timedelta(days=30)).replace(microsecond=0)
s3.put_object_retention(Bucket=BUCKET, Key="records/contract.pdf", Retention={"Mode": "COMPLIANCE", "RetainUntilDate": until})
got = s3.get_object_retention(Bucket=BUCKET, Key="records/contract.pdf")["Retention"]
check("PutObjectRetention COMPLIANCE 30 days", got["Mode"] == "COMPLIANCE" and abs((got["RetainUntilDate"] - until).total_seconds()) < 2)
try: s3.delete_object(Bucket=BUCKET, Key="records/contract.pdf"); deleted = True
except ClientError as e: deleted = False; code = e.response["Error"]["Code"]
check("A COMPLIANCE-locked file can't be deleted — not even by admin", not deleted)
try:
    s3.put_object_retention(Bucket=BUCKET, Key="records/contract.pdf", Retention={"Mode": "COMPLIANCE", "RetainUntilDate": until - dt.timedelta(days=10)}); shortened = True
except ClientError: shortened = False
check("…and its date can't be shortened", not shortened)
_, f = api("GET", f"/search?q=contract.pdf"); fid = next((x["id"] for x in (f if isinstance(f, list) else f.get("items", [])) if x.get("name") == "contract.pdf"), None)
c, _ = api("DELETE", f"/files/{fid}") if fid else (0, None)
check("…the web app refuses it too (same rule everywhere)", c == 403, str(c))

s3.put_object(Bucket=BUCKET, Key="records/evidence.pdf", Body=b"%PDF-1.4 evidence")
s3.put_object_legal_hold(Bucket=BUCKET, Key="records/evidence.pdf", LegalHold={"Status": "ON"})
check("Legal hold ON", s3.get_object_legal_hold(Bucket=BUCKET, Key="records/evidence.pdf")["LegalHold"]["Status"] == "ON")
try: s3.delete_object(Bucket=BUCKET, Key="records/evidence.pdf"); held = False
except ClientError: held = True
check("…blocks deletion", held)
s3.put_object_legal_hold(Bucket=BUCKET, Key="records/evidence.pdf", LegalHold={"Status": "OFF"})
s3.delete_object(Bucket=BUCKET, Key="records/evidence.pdf")
check("…lifting it allows deletion", "records/evidence.pdf" not in [o["Key"] for o in s3.list_objects_v2(Bucket=BUCKET, Prefix="records/").get("Contents", [])])

# ── basket default lock → every new file locked
_, st = api("GET", f"/baskets/{SYS}/settings")
st["policy"]["lock_days"] = 7; st["policy"]["lock_mode"] = "GOVERNANCE"
c, _ = api("PUT", f"/baskets/{SYS}/settings", st)
s3.put_object(Bucket=BUCKET, Key="auto/new.txt", Body=b"auto")
auto = s3.get_object_retention(Bucket=BUCKET, Key="auto/new.txt")["Retention"]
cfg = s3.get_object_lock_configuration(Bucket=BUCKET)["ObjectLockConfiguration"]
check("Basket default lock: new files locked 7 days (GOVERNANCE)", c == 200 and auto["Mode"] == "GOVERNANCE", f"{c} {auto['Mode']}")
check("GetObjectLockConfiguration shows the default", cfg["Rule"]["DefaultRetention"] == {"Mode": "GOVERNANCE", "Days": 7})

# ── webhooks: receiver gets a signed event
got_events = []
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(s):
        body = s.rfile.read(int(s.headers["content-length"])); got_events.append((s.headers.get("x-filehub-event"), s.headers.get("x-filehub-signature"), body))
        s.send_response(200); s.end_headers()
    def log_message(s, *a): pass
srv = http.server.HTTPServer(("127.0.0.1", 8797), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
c, hook = api("POST", "/webhooks", {"name": f"test {tag}", "url": "http://127.0.0.1:8797/hook", "system_id": SYS, "events": ["file.uploaded"]})
s3.put_object(Bucket=BUCKET, Key="hooks/arrived.txt", Body=b"hello")
for _ in range(20):
    if got_events: break
    time.sleep(0.5)
ev = got_events[0] if got_events else (None, None, b"{}")
sig_ok = bool(hook and isinstance(hook, dict)) and ev[1] == "sha256=" + hmac.new(hook["secret"].encode(), ev[2], hashlib.sha256).hexdigest()
check("Webhook: receiver gets file.uploaded within seconds, HMAC-signed", ev[0] == "file.uploaded" and sig_ok and b"arrived.txt" in ev[2], f"event={ev[0]} sig_ok={sig_ok}")
srv.shutdown()
print(f"\n{sum(R)}/{len(R)} passed")
