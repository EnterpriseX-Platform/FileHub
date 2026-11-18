-- 22 ก.ย. 2569 (NEB): รองรับการเข้าใช้งานด้วยตัวตนจากขอบนอก (oauth2-proxy)
-- โดยไม่ต้องล็อกอินซ้ำและไม่ต้องขอ API token
--
-- ผู้ใช้ที่ถูกสร้างอัตโนมัติจากขอบนอกต้องจำได้ว่า "สังกัดหน่วยงานไหน / ระบบไหน"
-- เพื่อให้ตอนอัปโหลดไม่ต้องกรอก system_id/org_id เองทุกครั้ง และติดแท็กให้อัตโนมัติ

ALTER TABLE users ADD COLUMN IF NOT EXISTS default_system_id TEXT REFERENCES systems(id);
ALTER TABLE users ADD COLUMN IF NOT EXISTS default_org_id    TEXT REFERENCES orgs(id);
-- ที่มาของบัญชี: 'local' = ตั้งรหัสผ่านในระบบนี้ · 'edge' = มาจากตัวตนของขอบนอก
ALTER TABLE users ADD COLUMN IF NOT EXISTS source TEXT NOT NULL DEFAULT 'local';

CREATE INDEX IF NOT EXISTS idx_users_source ON users(source);
