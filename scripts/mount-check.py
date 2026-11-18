"""Mounted-folder baskets: a basket that mirrors a folder on disk.

Backend needs MOUNT_ROOTS=<dir> (MOUNT_DIR below must be inside it) and the
storage root in FH_STORE (to check that old versions are kept).
  FH_BASE=http://127.0.0.1:8190 MOUNT_DIR=/…/fh-mounts FH_STORE=/…/fh-bench-storage python3.11 scripts/mount-check.py
"""
import json, os, shutil, time, uuid, urllib.request, urllib.error, http.cookiejar

B = os.environ.get("FH_BASE", "http://127.0.0.1:8190") + "/fh/api"
ROOT = os.environ["MOUNT_DIR"]
STORE = os.environ.get("FH_STORE", "")
R = []
def check(n, ok, d=""): R.append(bool(ok)); print("PASS" if ok else "FAIL", n, "—", d)

class C:
    def __init__(self, email, pw):
        self.op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
        self.req("POST", "/auth/login", {"email": email, "password": pw})
    def req(self, m, p, body=None, files=None, fields=None, raw=False):
        h, data = {}, None
        if files is not None:
            bd = uuid.uuid4().hex; parts = [f'--{bd}\r\nContent-Disposition: form-data; name="{k}"\r\n\r\n{v}\r\n'.encode() for k, v in (fields or {}).items()]
            for n, c in files: parts.append(f'--{bd}\r\nContent-Disposition: form-data; name="file"; filename="{n}"\r\n\r\n'.encode() + c + b"\r\n")
            data = b"".join(parts) + f"--{bd}--\r\n".encode(); h["content-type"] = f"multipart/form-data; boundary={bd}"
        elif body is not None:
            data = json.dumps(body).encode(); h["content-type"] = "application/json"
        try: r = self.op.open(urllib.request.Request(B + p, data=data, method=m, headers=h)); c, b = r.status, r.read()
        except urllib.error.HTTPError as e: c, b = e.code, e.read()
        if raw: return c, b
        try: return c, json.loads(b) if b else None
        except Exception: return c, b

admin = C("admin@acme.go.th", "admin123"); viewer = C("viewer@acme.go.th", "viewer123")
tag = time.strftime("%H%M%S")
d = os.path.join(ROOT, f"share-{tag}")
os.makedirs(os.path.join(d, "contracts"))
open(os.path.join(d, "budget.pdf"), "wb").write(b"%PDF-1.4 original budget")
open(os.path.join(d, "contracts", "lease.txt"), "w").write("lease v1")
open(os.path.join(d, ".hidden"), "w").write("x")
os.symlink("/etc/passwd", os.path.join(d, "passwd-link"))

_, sysb = admin.req("POST", "/systems", {"name": f"Shared drive {tag}", "bucket": f"shared-{tag}", "tone": "slate"})
SYS = sysb["id"]

# 1. only folders inside MOUNT_ROOTS
c1, _ = admin.req("PUT", f"/baskets/{SYS}/mount", {"path": "/etc"})
c2, _ = admin.req("PUT", f"/baskets/{SYS}/mount", {"path": d + "/../.."})
c3, _ = viewer.req("PUT", f"/baskets/{SYS}/mount", {"path": d})
check("Only admins, only folders inside MOUNT_ROOTS (no /etc, no ..)", c1 == 400 and c2 == 400 and c3 == 403, f"/etc {c1} · .. {c2} · viewer {c3}")

evil = os.path.join(ROOT, f"evil-{tag}"); os.symlink("/etc", evil)
c4, _ = admin.req("PUT", f"/baskets/{SYS}/mount", {"path": evil})
check("A symlink inside MOUNT_ROOTS that points outside is refused", c4 == 400, str(c4))
os.remove(evil)

# 2. mount → first scan brings the files in
c, m = admin.req("PUT", f"/baskets/{SYS}/mount", {"path": d})
check("Mounting scans the folder: 2 files in (hidden file and symlink skipped)", c == 200 and m["scan"]["added"] == 2, f"{c} {m}")
_, files = admin.req("GET", f"/files?system_id={SYS}&limit=50")
names = sorted(f["name"] for f in files)
check("…files listed in the basket", names == ["budget.pdf", "lease.txt"], str(names))
_, folders = admin.req("GET", f"/folders?system_id={SYS}")
check("…sub-folder becomes a basket folder", any(f["name"] == "contracts" for f in folders), str([f["name"] for f in folders]))
bud = next(f for f in files if f["name"] == "budget.pdf")
c, body = admin.req("GET", f"/files/{bud['id']}/download", raw=True)
check("Download reads the file from the folder", c == 200 and body == b"%PDF-1.4 original budget", f"{c} {body[:30]!r}")

# 3. uploads land in the folder under their real name
c, up = admin.req("POST", "/files", files=[("report.pdf", b"%PDF-1.4 from filehub")], fields={"system_id": SYS})
c2, up2 = admin.req("POST", "/files", files=[("report.pdf", b"%PDF-1.4 second")], fields={"system_id": SYS})
on_disk = sorted(x for x in os.listdir(d) if not x.startswith("."))
check("Upload is written into the folder under its real name (taken name gets a number)", c == 200 and c2 == 200 and "report.pdf" in on_disk and "report (2).pdf" in on_disk, str(on_disk))
check("…as a plain file other programs can read", open(os.path.join(d, "report.pdf"), "rb").read() == b"%PDF-1.4 from filehub")

_, fo = admin.req("POST", "/folders", {"name": f"planted-{tag}", "system_id": SYS})
os.symlink("/tmp", os.path.join(d, f"planted-{tag}"))
c, _ = admin.req("POST", "/files", files=[("x.pdf", b"%PDF-1.4 x")], fields={"system_id": SYS, "folder_id": fo["id"]})
check("Upload into a folder that is really a planted symlink is refused", c == 400, str(c))
os.remove(os.path.join(d, f"planted-{tag}"))

# 4. new version: the folder holds the current one, the old one is kept by FileHub
c, _ = admin.req("POST", f"/files/{bud['id']}/versions", files=[("budget.pdf", b"%PDF-1.4 budget v2")])
_, vers = admin.req("GET", f"/files/{bud['id']}/versions")
kept = [v.get("object_key", "") for v in (vers or [])]
check("New version: folder has the new bytes, old version kept in FileHub storage", c == 200 and open(os.path.join(d, "budget.pdf"), "rb").read() == b"%PDF-1.4 budget v2"
      and any(k.startswith(f"shared-{tag}/") for k in kept) and (not STORE or any(os.path.exists(os.path.join(STORE, k)) for k in kept)), f"{c} {kept}")

# 5. changes made by other programs
time.sleep(1.1)
open(os.path.join(d, "contracts", "lease.txt"), "w").write("lease v2 — changed outside")
open(os.path.join(d, "contracts", "new.txt"), "w").write("added outside")
os.remove(os.path.join(d, "report (2).pdf"))
c, sc = admin.req("POST", f"/baskets/{SYS}/scan")
check("Scan picks up outside changes: 1 added, ≥1 updated, 1 removed", c == 200 and sc["added"] == 1 and sc["updated"] >= 1 and sc["removed"] == 1, f"{c} {sc}")
_, files = admin.req("GET", f"/files?system_id={SYS}&limit=50")
lease = next(f for f in files if f["name"] == "lease.txt")
c, body = admin.req("GET", f"/files/{lease['id']}/download", raw=True)
check("…changed file serves the new content", body.decode() == "lease v2 — changed outside", body[:40])
check("…removed file left the basket", not any(f["id"] == up2["id"] for f in files))

# 6. Trash in FileHub isn't undone by the next scan
admin.req("DELETE", f"/files/{lease['id']}")
c, sc = admin.req("POST", f"/baskets/{SYS}/scan")
_, files = admin.req("GET", f"/files?system_id={SYS}&limit=50")
check("A file moved to Trash stays there after the next scan", not any(f["id"] == lease["id"] for f in files), f"{sc}")

c, _ = viewer.req("POST", f"/baskets/{SYS}/scan")
check("Scan needs Manage on the basket", c in (403, 404), str(c))

# cleanup
admin.req("PUT", f"/baskets/{SYS}/mount", {"path": None})
shutil.rmtree(d, ignore_errors=True)
print(f"\n{sum(R)}/{len(R)} passed")
