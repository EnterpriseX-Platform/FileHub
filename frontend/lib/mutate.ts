// ─── ขอบนอกของ NEB ปล่อยเฉพาะ GET/POST ──────────────────────────────────
// Cloudflare หน้า uat-neb.bb.go.th ตอบ 403 ให้ PATCH/PUT/DELETE ตั้งแต่ยังไม่ถึงแอป
// และกติกาของโครงการคือห้ามไปขอทีม WAF เปิดเมธอดให้ ⇒ ส่งเป็น POST แล้วบอก
// เมธอดจริงทางเฮดเดอร์ ซึ่งฝั่งหลังบ้านมีชั้นแปลงกลับให้แล้ว (method_override)
export function mutate(url: string, method: "DELETE" | "PATCH" | "PUT", init: RequestInit = {}) {
  const headers = new Headers(init.headers as HeadersInit | undefined);
  headers.set("x-http-method-override", method);
  return fetch(url, { ...init, method: "POST", headers });
}
