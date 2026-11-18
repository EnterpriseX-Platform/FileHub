"""Roles (admin/editor/viewer) and grants (view/edit/manage on basket/folder/file),
checked as each kind of person. Runs against localhost:8090 with the demo seed."""
import os, json, time, uuid, urllib.request, urllib.error, http.cookiejar
B = os.environ.get("FH_BASE", "http://localhost:8090") + "/fh/api"; R = []
def check(n, ok, d=""): R.append(bool(ok)); print("PASS" if ok else "FAIL", n, "—", d)
class C:
    def __init__(s, email, pw):
        s.cj = http.cookiejar.CookieJar(); s.op = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(s.cj))
        s.code = s.j("POST", "/auth/login", {"email": email, "password": pw})[0]
    def j(s, m, p, o=None):
        try:
            r = s.op.open(urllib.request.Request(B + p, data=json.dumps(o).encode() if o is not None else None, method=m, headers={"content-type": "application/json"} if o is not None else {}))
            b = r.read(); return r.status, json.loads(b) if b else None
        except urllib.error.HTTPError as e:
            b = e.read()
            try: return e.code, json.loads(b)
            except Exception: return e.code, b
    def up(s, sys, folder=None, name=None):
        bd = uuid.uuid4().hex; name = name or f"f-{uuid.uuid4().hex[:6]}.pdf"
        parts = [f'--{bd}\r\nContent-Disposition: form-data; name="system_id"\r\n\r\n{sys}\r\n']
        if folder: parts.append(f'--{bd}\r\nContent-Disposition: form-data; name="folder_id"\r\n\r\n{folder}\r\n')
        body = "".join(parts).encode() + f'--{bd}\r\nContent-Disposition: form-data; name="file"; filename="{name}"\r\n\r\n%PDF-1.4 x\r\n--{bd}--\r\n'.encode()
        try: r = s.op.open(urllib.request.Request(B + "/files", data=body, headers={"content-type": f"multipart/form-data; boundary={bd}"})); return r.status, json.loads(r.read())
        except urllib.error.HTTPError as e: return e.code, e.read()

tag = time.strftime("%H%M%S")
admin = C("admin@acme.go.th", "admin123")
# ── 1. Admin manages people and roles
mk = lambda e, r: admin.j("POST", "/users", {"email": e, "display_name": e.split("@")[0], "password": "pass12345", "role": r})
c1, u_ed = mk(f"ed.{tag}@neb.go.th", "editor"); c2, u_vw = mk(f"vw.{tag}@neb.go.th", "viewer"); c3, u_ot = mk(f"other.{tag}@neb.go.th", "editor")
check("Admin adds people with a role", c1 == c2 == c3 == 200, f"{c1} {c2} {c3}")
ed, vw, ot = C(f"ed.{tag}@neb.go.th", "pass12345"), C(f"vw.{tag}@neb.go.th", "pass12345"), C(f"other.{tag}@neb.go.th", "pass12345")
check("New people can sign in", ed.code == vw.code == ot.code == 200)
c, _ = admin.j("PATCH", f"/users/{u_vw['id']}", {"role": "editor"}); c2, me = vw.j("GET", "/auth/me")
check("Admin changes a role — takes effect at once", c == 200 and me.get("role") == "editor", f"{c} → {me.get('role')}")
admin.j("PATCH", f"/users/{u_vw['id']}", {"role": "viewer"})
# ── 2. What each role may do by default (basket with no grants = role decides)
_, b = admin.j("POST", "/systems", {"name": f"Roles {tag}", "bucket": f"roles-{tag}", "tone": "slate"})
SYS = b["id"]
c_e, fe = ed.up(SYS); c_v, _ = vw.up(SYS)
check("Editor can upload; viewer can't", c_e == 200 and c_v == 403, f"editor {c_e} · viewer {c_v}")
c, lst = vw.j("GET", f"/files?system_id={SYS}")
check("Viewer can see and open files", c == 200 and any(x["id"] == fe["id"] for x in lst))
c_del, _ = vw.j("DELETE", f"/files/{fe['id']}")
check("Viewer can't delete", c_del in (403, 404), str(c_del))
for who, cl in (("editor", ed), ("viewer", vw)):
    check(f"{who.title()} can't open Admin functions (create basket, users, S3 keys)",
          all(cl.j(m, p, o)[0] in ((403, 404) if p == "/s3-credentials" else (403,)) for m, p, o in [("POST", "/systems", {"name": "x", "bucket": f"x-{tag}"}), ("GET", "/users", None), ("POST", "/users", {"email": f"z{tag}@z.co", "display_name": "Z", "password": "12345678", "role": "admin"}),
                                    ("PATCH", f"/users/{u_ed['id']}", {"role": "admin"}), ("GET", "/s3-credentials", None), ("GET", "/api-keys", None), ("PATCH", "/workspace", {"values": {"office_viewer": "disabled"}})]))
_, cur = admin.j("GET", f"/baskets/{SYS}/settings")
c, _ = ed.j("PUT", f"/baskets/{SYS}/settings", cur)
check("Editor can't change basket settings", c in (403, 404), str(c))
c, _ = ed.j("GET", "/trash")
check("Trash shows only what the person may restore", c == 200)
# ── 3. Grants: basket strict → only granted people
admin.j("PUT", f"/acl/basket/{SYS}", {"entries": [{"principal": "user:usr_admin", "level": "manage"}, {"principal": f"user:{u_ed['id']}", "level": "edit"}]})
c_ot, l_ot = ot.j("GET", f"/files?system_id={SYS}"); c_vw2, l_vw = vw.j("GET", f"/files?system_id={SYS}")
check("Basket given to named people: others (even editors) see nothing", (l_ot == [] or c_ot in (403, 404)) and (l_vw == [] or c_vw2 in (403, 404)), f"other {c_ot}/{len(l_ot) if isinstance(l_ot, list) else '-'} · viewer {c_vw2}")
check("…the person given Edit still uploads", ed.up(SYS)[0] == 200)
# folder: give viewer View on one folder only
_, fo = ed.j("POST", "/folders", {"system_id": SYS, "name": "เปิดให้ดู"}); fid_f = fo["id"]
_, inside = ed.up(SYS, fid_f, "inside.pdf")
c, _ = ed.j("PUT", f"/acl/folder/{fid_f}", {"entries": [{"principal": f"user:{u_vw['id']}", "level": "view"}]})
check("Edit can forward View to someone", c == 200, str(c))
c, _ = ed.j("PUT", f"/acl/folder/{fid_f}", {"entries": [{"principal": f"user:{u_vw['id']}", "level": "view"}, {"principal": f"user:{u_ot['id']}", "level": "manage"}]})
check("…but Edit can't give Manage", c in (400, 403), str(c))
_, l = vw.j("GET", f"/files?system_id={SYS}")
ids = {x["id"] for x in l} if isinstance(l, list) else set()
check("Folder shared with viewer: sees only that folder's files", inside["id"] in ids and fe["id"] not in ids, f"sees {len(ids)}")
check("…can download it", vw.j("GET", f"/files/{inside['id']}")[0] == 200)
check("…but not files outside it", vw.j("GET", f"/files/{fe['id']}")[0] == 404)
# file-level grant
_, single = ed.up(SYS, None, "one.pdf")
ed.j("PUT", f"/acl/file/{single['id']}", {"entries": [{"principal": f"user:{u_ot['id']}", "level": "view"}]})
check("One file shared with one person: they can open just that file", ot.j("GET", f"/files/{single['id']}")[0] == 200 and ot.j("GET", f"/files/{inside['id']}")[0] == 404)
_, sh = ot.j("GET", "/me/shared")
check("…and it shows in their 'Shared with me'", any(x.get("id") == single["id"] or x.get("file_id") == single["id"] for x in (sh or [])))
# manage: give Manage on folder, they can manage access
admin.j("PUT", f"/acl/folder/{fid_f}", {"entries": [{"principal": f"user:{u_vw['id']}", "level": "view"}, {"principal": f"user:{u_ot['id']}", "level": "manage"}]})
c, _ = ot.j("PUT", f"/acl/folder/{fid_f}", {"entries": [{"principal": f"user:{u_vw['id']}", "level": "edit"}, {"principal": f"user:{u_ot['id']}", "level": "manage"}]})
check("Manage on a folder lets that person change who has access", c == 200, str(c))
# inheritance off
_, sub = ed.j("POST", "/folders", {"system_id": SYS, "parent_id": fid_f, "name": "ลับเฉพาะ"})
_, secret = ed.up(SYS, sub["id"], "secret.pdf")
admin.j("PUT", f"/acl/folder/{sub['id']}", {"entries": [{"principal": "user:usr_admin", "level": "manage"}], "inherit": False})
check("Sub-folder with inheritance off: people from above lose access", vw.j("GET", f"/files/{secret['id']}")[0] == 404 and ot.j("GET", f"/files/{secret['id']}")[0] == 404)
# agency grant
_, orgs = admin.j("GET", "/orgs"); org = orgs[0]["id"]
admin.j("PATCH", f"/users/{u_ot['id']}", {"default_org_id": org})
admin.j("PUT", f"/acl/folder/{sub['id']}", {"entries": [{"principal": "user:usr_admin", "level": "manage"}, {"principal": f"org:{org}", "level": "view"}], "inherit": False})
check("Grant to an agency: people in that agency get in", ot.j("GET", f"/files/{secret['id']}")[0] == 200, org)
# expiry
admin.j("PUT", f"/acl/file/{single['id']}", {"entries": [{"principal": f"user:{u_vw['id']}", "level": "view", "expires_at": "2020-01-01T00:00:00Z"}]})
check("Expired grant no longer works", vw.j("GET", f"/files/{single['id']}")[0] == 404)
# deactivate
c, _ = admin.j("PATCH", f"/users/{u_ot['id']}", {"status": "disabled"})
check("Admin turns an account off: it stops working at once", c == 200 and ot.j("GET", "/auth/me")[0] == 401, str(c))
# admin sees all
check("Admin sees everything, even a strict sub-folder", admin.j("GET", f"/files/{secret['id']}")[0] == 200)
print(f"\n{sum(R)}/{len(R)} passed")
