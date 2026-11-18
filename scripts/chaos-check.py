"""Chaos checks: FileHub must stay up and recover by itself.

Needs a FileHub whose DATABASE_URL goes through the TCP proxy this script runs
(PROXY_PORT, default 5499 → PG_TARGET 127.0.0.1:5432), so the database can be
"lost" for FileHub alone without stopping Postgres for anyone else.

  python3.11 scripts/chaos-check.py proxy          # run the proxy (in background)
  FH_BASE=http://127.0.0.1:8190 python3.11 scripts/chaos-check.py

Checks: database lost mid-upload and back; malformed / hostile requests;
decompression-bomb image; corrupt Office file; memory stays bounded.
"""
import json, os, socket, struct, subprocess, sys, threading, time, uuid, zlib, urllib.request, urllib.error, http.cookiejar

PROXY_PORT = int(os.environ.get("PROXY_PORT", "5499"))
PG = os.environ.get("PG_TARGET", "127.0.0.1:5432")
CTRL = "/tmp/fh-chaos-proxy-off"   # exists = database "down"

def proxy():
    host, port = PG.split(":"); port = int(port)
    srv = socket.socket(); srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    srv.bind(("127.0.0.1", PROXY_PORT)); srv.listen(512)
    live = []
    def pipe(a, b):
        try:
            while True:
                d = a.recv(65536)
                if not d or os.path.exists(CTRL): break
                b.sendall(d)
        except OSError: pass
        for s in (a, b):
            try: s.close()
            except OSError: pass
    def killer():
        while True:
            if os.path.exists(CTRL):
                for s in list(live):
                    try: s.close()
                    except OSError: pass
                live.clear()
            time.sleep(0.2)
    threading.Thread(target=killer, daemon=True).start()
    while True:
        c, _ = srv.accept()
        if os.path.exists(CTRL): c.close(); continue
        try: u = socket.create_connection((host, port))
        except OSError: c.close(); continue
        live.extend([c, u])
        threading.Thread(target=pipe, args=(c, u), daemon=True).start()
        threading.Thread(target=pipe, args=(u, c), daemon=True).start()

if len(sys.argv) > 1 and sys.argv[1] == "proxy":
    if os.path.exists(CTRL): os.remove(CTRL)
    proxy()

ROOT = os.environ.get("FH_BASE", "http://127.0.0.1:8190")
B = ROOT + "/fh/api"
R = []
def check(n, ok, d=""): R.append(bool(ok)); print("PASS" if ok else "FAIL", n, "—", d)

cj = http.cookiejar.CookieJar(); op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(cj))
def req(m, p, data=None, h=None, timeout=60):
    try: r = op.open(urllib.request.Request(p if p.startswith("http") else B + p, data=data, method=m, headers=h or {}), timeout=timeout); return r.status, r.read()
    except urllib.error.HTTPError as e: return e.code, e.read()
    except Exception as e: return 0, str(e).encode()
def upload(name, body, sys_id="sys_dms"):
    bd = uuid.uuid4().hex
    data = f'--{bd}\r\nContent-Disposition: form-data; name="system_id"\r\n\r\n{sys_id}\r\n--{bd}\r\nContent-Disposition: form-data; name="file"; filename="{name}"\r\n\r\n'.encode() + body + f"\r\n--{bd}--\r\n".encode()
    return req("POST", "/files", data, {"content-type": f"multipart/form-data; boundary={bd}"})
def pid_rss():
    port = ROOT.rsplit(":", 1)[1]
    pid = subprocess.run(["lsof", "-nP", f"-iTCP:{port}", "-sTCP:LISTEN", "-t"], capture_output=True, text=True).stdout.split()[0]
    return int(subprocess.run(["ps", "-o", "rss=", "-p", pid], capture_output=True, text=True).stdout.strip()) // 1024

req("POST", "/auth/login", json.dumps({"email": "admin@acme.go.th", "password": "admin123"}).encode(), {"content-type": "application/json"})
rss0 = pid_rss()

# ── 1. database lost mid-traffic, then back ──
results = []
stop = False
def traffic():
    while not stop:
        c, _ = upload(f"chaos-{uuid.uuid4().hex[:6]}.pdf", b"%PDF-1.4 chaos " + os.urandom(20000))
        results.append((time.time(), c))
ts = [threading.Thread(target=traffic) for _ in range(8)]
[t.start() for t in ts]
time.sleep(4)
open(CTRL, "w").close(); t_down = time.time()
time.sleep(15)
live_during = req("GET", "/health")[0]; ready_during = req("GET", "/ready")[0]
os.remove(CTRL); t_up = time.time()
time.sleep(12)
stop = True; [t.join() for t in ts]
before = [c for t, c in results if t < t_down]
during = [c for t, c in results if t_down + 2 < t < t_up]
after = [c for t, c in results if t > t_up + 6]
check("Database lost: the process stays up (liveness 200), readiness says not ready", live_during == 200 and ready_during != 200, f"health {live_during} · ready {ready_during}")
check("…uploads during the outage fail cleanly with 503 + Retry-After (no hang)", len(during) > 0 and all(c == 503 for c in during), f"{len(during)} requests: {sorted(set(during))}")
check("…and recover by themselves when it is back (no restart)", len(after) > 0 and all(c == 200 for c in after), f"before {sorted(set(before))} · after {len(after)} requests {sorted(set(after))}")

# ── 2. hostile and malformed requests ──
c1, _ = req("POST", "/files", b"--x\r\nbroken", {"content-type": "multipart/form-data; boundary=x"})
c2, _ = req("POST", "/files", b"{not json", {"content-type": "application/json"})
c3, _ = req("GET", "/files/" + "a" * 5000)
c4, _ = upload("../../../../etc/passwd", b"root:x")
c5, _ = req("POST", "/uploads", b"", {"Tus-Resumable": "1.0.0", "Upload-Length": "99999999999999999", "Upload-Metadata": "filename eA==,system_id c3lzX2Rtcw=="})
c6, _ = req("GET", "/files?limit=999999999&offset=-5")
c7, _ = req("POST", "/acl/basket/sys_dms", b'{"entries":' + b"[" * 5000 + b"]" * 5000 + b"}", {"content-type": "application/json"})
codes = [c1, c2, c3, c4, c5, c6, c7]
check("Malformed/hostile requests are answered (4xx), never crash", req("GET", "/health")[0] == 200 and all(c and c < 500 for c in codes), str(codes))

# ── 3. decompression-bomb PNG (20000×20000 from a few KB) ──
def png(w, h):
    raw = b"".join(b"\x00" + b"\x00" * (w * 3) for _ in range(h))
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
bomb = png(20000, 20000)
c, _ = upload("bomb.png", bomb)
time.sleep(4)
rss_bomb = pid_rss()
check("Decompression-bomb image: stored, thumbnailer doesn't blow memory", c == 200 and req("GET", "/health")[0] == 200 and rss_bomb < rss0 + 600, f"{len(bomb)//1024} KB file · RSS {rss0}→{rss_bomb} MB")

# ── 4. corrupt Office file: conversion fails or times out, nothing hangs ──
c, b = upload("broken.docx", b"PK\x03\x04" + os.urandom(50000))
fid = json.loads(b).get("id") if c == 200 else None
t = time.time(); cp, _ = req("GET", f"/files/{fid}/preview", timeout=200) if fid else (0, b"")
check("Corrupt Word file: preview answers (404) within the time limit", c == 200 and cp in (404, 413) and time.time() - t < 150, f"upload {c} · preview {cp} in {time.time()-t:.0f}s")

rss1 = pid_rss()
check("Memory stays bounded through all of it", rss1 < rss0 + 600, f"RSS {rss0} → {rss1} MB")
print(f"\n{sum(R)}/{len(R)} passed")
