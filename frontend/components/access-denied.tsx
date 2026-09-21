/// จอ "ไม่มีสิทธิ์" — ขึ้นเมื่อหลังบ้านตอบ 403 คือผู้ใช้ล็อกอิน NEB มาแล้วจริง
/// แต่ไม่มีรหัสสิทธิ์ที่กำหนดไว้ใน IAM-X สำหรับคลังไฟล์
///
/// ทำไมต้องมีจอนี้: ถ้าปล่อยให้เป็นจอว่างหรือเด้งไปหน้า login ของ FileHub เอง
/// ผู้ใช้จะเข้าใจว่าระบบพัง แล้วโทรมาถามทีละคน — บอกให้ชัดว่าเป็นเรื่องสิทธิ์
/// และต้องไปขอที่ไหนจะจบเร็วกว่า
export function AccessDenied({ email }: { email?: string }) {
  return (
    <div
      style={{
        minHeight: "100vh",
        display: "grid",
        placeItems: "center",
        padding: 24,
        background: "var(--bg-subtle)",
      }}
    >
      <div className="card" style={{ maxWidth: 520, padding: 28, textAlign: "center" }}>
        <div
          style={{
            width: 48, height: 48, margin: "0 auto 16px",
            borderRadius: 12, display: "grid", placeItems: "center",
            background: "var(--danger-soft)", border: "1px solid var(--danger-border)",
            color: "var(--danger)", fontSize: 22, fontWeight: 700,
          }}
          aria-hidden
        >
          !
        </div>
        <h1 className="t-lg t-semibold" style={{ margin: "0 0 8px" }}>
          ไม่มีสิทธิ์เข้าใช้งานคลังไฟล์
        </h1>
        <p className="t-sm t-muted" style={{ margin: "0 0 4px", lineHeight: 1.7 }}>
          บัญชีของคุณเข้าสู่ระบบ NEB เรียบร้อยแล้ว แต่ยังไม่ได้รับสิทธิ์ให้ใช้งานคลังไฟล์กลาง
        </p>
        {email && (
          <p className="t-xs t-subtle" style={{ margin: "0 0 16px" }}>
            บัญชีที่ใช้อยู่: {email}
          </p>
        )}
        <p className="t-sm t-muted" style={{ margin: "0 0 20px", lineHeight: 1.7 }}>
          หากจำเป็นต้องใช้งาน กรุณาติดต่อผู้ดูแลระบบเพื่อขอสิทธิ์ผ่านระบบบริหารจัดการผู้ใช้ (IAM-X)
        </p>
        <a href="/app/" className="btn primary" style={{ textDecoration: "none" }}>
          กลับหน้าพอร์ทัล
        </a>
      </div>
    </div>
  );
}
