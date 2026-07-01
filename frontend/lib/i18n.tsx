"use client";

import * as React from "react";

// Lightweight, dependency-free i18n for Thai/English (MEA TOR 5.3.1.12).
// Mirrors theme-context: the active locale is read from the `fh-locale` cookie
// SERVER-SIDE in app/layout.tsx and passed as initialLocale, so the first
// server render and client hydration agree (no flash, no mismatch). Strings are
// key-based (ICU-style) so this can be swapped for react-intl later with the
// same call sites. Missing keys fall back: locale → en → the key itself.

export type Locale = "en" | "th";

const CATALOG: Record<Locale, Record<string, string>> = {
  en: {
    "nav.dashboard": "Dashboard",
    "nav.ask": "Ask",
    "nav.files": "All files",
    "nav.search": "Search",
    "nav.activity": "Activity",
    "nav.reports": "Reports",
    "rep.active": "Active",
    "rep.inactive": "Inactive",
    "rep.retention": "Retention",
    "rep.deleted": "Deleted",
    "rep.exportStatus": "Export status (CSV)",
    "rep.exportAudit": "Export audit log (CSV)",
    "rep.docState": "Document status",
    "nav.views": "Views",
    "nav.savedViews": "Saved views",
    "nav.more": "More",
    "nav.upload": "Upload",
    "nav.shared": "Shared",
    "nav.archive": "Archive",
    "nav.trash": "Trash",
    "nav.settings": "Settings",
    "cmd.goto": "Go to",
    "cmd.noMatch": "No matching pages.",
    "cmd.searchFiles": "Search files",
    "cmd.findMeaning": "Find by meaning",
    "cmd.ask": "Ask",
    "eday.nav.home": "Home",
    "eday.nav.myfiles": "My files",
    "eday.nav.shared": "Shared",
    "eday.subLeftOff": "Here's where you left off.",
    "eday.subFirst": "Upload your first document to get started.",
    "eday.needsReview": "Needs your review",
    "eday.recent": "Recent files",
    "eday.yourAreas": "Your areas",
    "eday.viewAll": "View all",
    "eday.review": "Review",
    "eday.allCaughtUp": "You're all caught up — nothing waiting on you.",
    "eday.noFiles": "Nothing here yet. Upload a file to get started.",
    "eday.askIntro": "Ask a question about your documents",
    "eday.openFull": "Open full view",
    "eday.download": "Download",
    "eday.ask": "Ask",
    "eday.fullDetails": "Open full details",
    "eday.owner": "Owner",
    "eday.area": "Area",
    "eday.modified": "Modified",
    "eday.size": "Size",
    "eday.type": "Type",
    "eday.labels": "Labels",
    "eday.notFound": "This file isn't available.",
    "ask.title": "Ask your content",
    "ask.sub": "Answers with citations — not a folder you have to dig through.",
    "ask.placeholder": "Ask anything across your content…",
    "ask.button": "Ask",
    "ask.thinking": "Thinking…",
    "ask.answer": "Answer",
    "ask.grounded": "grounded",
    "ask.scoped": "scoped to your access",
    "ask.sources": "Sources",
    "search.title": "Search",
    "search.sub": "Meaning by default — finds what you mean, not just what you typed.",
    "search.placeholder": "Ask in your own words — e.g. agreements that auto-renew",
    "search.button": "Search",
    "search.searching": "Searching…",
    "search.prompt": "Type a question and hit search.",
    "search.noMatch": "No matches for “{q}”.",
    "search.results": "{n} results by meaning",
    "ask.ex1": "What do plants need to make food and what do they produce?",
    "ask.ex2": "What are the corporate tax filing obligations?",
    "rep.colName": "Name",
    "rep.colSystem": "System",
    "rep.colStatus": "Status",
    "rep.colState": "State",
    "rep.colModified": "Modified",
    "rep.noDocs": "No documents.",
    "ai.intelligence": "Intelligence",
    "ai.analyzing": "analyzing…",
    "ai.unavailable": "AI analysis unavailable.",
    "ai.analyzingFull": "Analyzing this file — summary and tags will appear shortly.",
    "ai.none": "Not analyzed yet.",
    "co.title": "Check-out",
    "co.byYou": "You have this checked out",
    "co.checkin": "Check in",
    "co.lockedBy": "Checked out by",
    "co.forceCheckin": "Force check-in",
    "co.checkout": "Check out",
    "co.available": "Available to check out",
  },
  th: {
    "nav.dashboard": "แดชบอร์ด",
    "nav.ask": "ถาม",
    "nav.files": "ไฟล์ทั้งหมด",
    "nav.search": "ค้นหา",
    "nav.activity": "กิจกรรม",
    "nav.reports": "รายงาน",
    "rep.active": "ใช้งาน",
    "rep.inactive": "ไม่ใช้งาน",
    "rep.retention": "รอลบ",
    "rep.deleted": "ลบถาวร",
    "rep.exportStatus": "ส่งออกสถานะ (CSV)",
    "rep.exportAudit": "ส่งออกบันทึกการใช้งาน (CSV)",
    "rep.docState": "สถานะเอกสาร",
    "nav.views": "มุมมอง",
    "nav.savedViews": "มุมมองที่บันทึก",
    "nav.more": "เพิ่มเติม",
    "nav.upload": "อัปโหลด",
    "nav.shared": "แชร์",
    "nav.archive": "คลังเอกสาร",
    "nav.trash": "ถังขยะ",
    "nav.settings": "ตั้งค่า",
    "cmd.goto": "ไปยัง",
    "cmd.noMatch": "ไม่พบหน้าที่ตรงกัน",
    "cmd.searchFiles": "ค้นหาไฟล์",
    "cmd.findMeaning": "ค้นหาตามความหมาย",
    "cmd.ask": "ถาม",
    "eday.nav.home": "หน้าแรก",
    "eday.nav.myfiles": "ไฟล์ของฉัน",
    "eday.nav.shared": "แชร์",
    "eday.subLeftOff": "ดำเนินการต่อจากที่ค้างไว้",
    "eday.subFirst": "อัปโหลดเอกสารแรกของคุณเพื่อเริ่มต้น",
    "eday.needsReview": "รอคุณตรวจสอบ",
    "eday.recent": "ไฟล์ล่าสุด",
    "eday.yourAreas": "พื้นที่ของคุณ",
    "eday.viewAll": "ดูทั้งหมด",
    "eday.review": "ตรวจสอบ",
    "eday.allCaughtUp": "เรียบร้อยแล้ว — ไม่มีสิ่งที่รอคุณอยู่",
    "eday.noFiles": "ยังไม่มีอะไรที่นี่ อัปโหลดไฟล์เพื่อเริ่มต้น",
    "eday.askIntro": "ถามคำถามเกี่ยวกับเอกสารของคุณ",
    "eday.openFull": "เปิดมุมมองเต็ม",
    "eday.download": "ดาวน์โหลด",
    "eday.ask": "ถาม",
    "eday.fullDetails": "เปิดรายละเอียดแบบเต็ม",
    "eday.owner": "เจ้าของ",
    "eday.area": "พื้นที่",
    "eday.modified": "แก้ไขล่าสุด",
    "eday.size": "ขนาด",
    "eday.type": "ประเภท",
    "eday.labels": "ป้ายกำกับ",
    "eday.notFound": "ไม่พบไฟล์นี้",
    "ask.title": "ถามจากเนื้อหาของคุณ",
    "ask.sub": "คำตอบพร้อมการอ้างอิง — ไม่ต้องค้นหาในโฟลเดอร์เอง",
    "ask.placeholder": "ถามอะไรก็ได้จากเนื้อหาของคุณ…",
    "ask.button": "ถาม",
    "ask.thinking": "กำลังคิด…",
    "ask.answer": "คำตอบ",
    "ask.grounded": "อ้างอิงจากเอกสาร",
    "ask.scoped": "จำกัดตามสิทธิ์ของคุณ",
    "ask.sources": "แหล่งอ้างอิง",
    "search.title": "ค้นหา",
    "search.sub": "ค้นหาตามความหมายเป็นค่าเริ่มต้น — พบสิ่งที่คุณต้องการ ไม่ใช่แค่คำที่พิมพ์",
    "search.placeholder": "พิมพ์ด้วยภาษาของคุณเอง — เช่น สัญญาที่ต่ออายุอัตโนมัติ",
    "search.button": "ค้นหา",
    "search.searching": "กำลังค้นหา…",
    "search.prompt": "พิมพ์คำถามแล้วกดค้นหา",
    "search.noMatch": "ไม่พบผลลัพธ์สำหรับ “{q}”",
    "search.results": "{n} ผลลัพธ์ตามความหมาย",
    "ask.ex1": "พืชต้องการอะไรในการสร้างอาหาร และผลิตอะไรออกมา?",
    "ask.ex2": "ภาระการยื่นภาษีของนิติบุคคลมีอะไรบ้าง?",
    "rep.colName": "ชื่อ",
    "rep.colSystem": "ระบบ",
    "rep.colStatus": "สถานะงาน",
    "rep.colState": "สถานะเอกสาร",
    "rep.colModified": "แก้ไขล่าสุด",
    "rep.noDocs": "ไม่มีเอกสาร",
    "ai.intelligence": "ปัญญาประดิษฐ์",
    "ai.analyzing": "กำลังวิเคราะห์…",
    "ai.unavailable": "ไม่สามารถวิเคราะห์ด้วย AI ได้",
    "ai.analyzingFull": "กำลังวิเคราะห์ไฟล์นี้ — สรุปและแท็กจะปรากฏในไม่ช้า",
    "ai.none": "ยังไม่ได้วิเคราะห์",
    "co.title": "เช็คเอาท์เอกสาร",
    "co.byYou": "คุณเช็คเอาท์เอกสารนี้อยู่",
    "co.checkin": "เช็คอิน",
    "co.lockedBy": "เช็คเอาท์โดย",
    "co.forceCheckin": "บังคับเช็คอิน",
    "co.checkout": "เช็คเอาท์",
    "co.available": "พร้อมให้เช็คเอาท์",
  },
};

type TParams = Record<string, string | number>;
type Ctx = { locale: Locale; setLocale: (l: Locale) => void; t: (k: string, params?: TParams) => string };
const I18nCtx = React.createContext<Ctx>({ locale: "en", setLocale: () => {}, t: (k) => k });

export const useI18n = () => React.useContext(I18nCtx);

export function I18nProvider({
  initialLocale = "en",
  children,
}: {
  initialLocale?: Locale;
  children: React.ReactNode;
}) {
  const [locale, setLocaleState] = React.useState<Locale>(initialLocale);
  const setLocale = React.useCallback((l: Locale) => {
    setLocaleState(l);
    try { document.cookie = `fh-locale=${l};path=/;max-age=31536000;samesite=lax`; } catch {}
    try { document.documentElement.lang = l; } catch {}
  }, []);
  const t = React.useCallback(
    (k: string, params?: TParams) => {
      const tpl = CATALOG[locale][k] ?? CATALOG.en[k] ?? k;
      if (!params) return tpl;
      return tpl.replace(/\{(\w+)\}/g, (m, name) =>
        name in params ? String(params[name]) : m);
    },
    [locale],
  );
  return <I18nCtx.Provider value={{ locale, setLocale, t }}>{children}</I18nCtx.Provider>;
}
