"use client";

import Link from "next/link";
import * as React from "react";

import { GlobalSearch } from "./global-search";
import { Ico } from "./icons";
import { MyDriveLink } from "./my-drive-link";
import { SideLabel, SideRow } from "./primitives";
import { SavedViewsList } from "./saved-views-list";
import { UserMenu } from "./user-menu";
import { fmtBytes, fmtCount } from "@/lib/format";
import { useSidebar } from "@/lib/sidebar-context";
import type { Org, System, DashboardStats } from "@/lib/api";

export type NavKey = "dashboard" | "files" | "activity" | "views" | "share" | "archive" | "trash" | "settings";

/// Sidebar is a pure synchronous component so it can render correctly inside
/// both server and client pages. Data is supplied entirely via props; pages
/// that want to show org rollups under the active system pass `orgs` in.
export function Sidebar({
  nav = "files",
  systemActive,
  orgActive,
  systems = [],
  orgs = [],
  stats,
}: {
  nav?: NavKey;
  systemActive?: string;
  orgActive?: string;
  systems?: System[];
  orgs?: Org[];
  stats?: DashboardStats | null;
}) {
  const { open, setOpen } = useSidebar();
  const activeSystemId = systemActive ?? systems[0]?.id;

  // จำนวนไฟล์ + ขนาดต่อถัง — จาก /api/stats
  const usage: Record<string, { files: number; bytes: number }> = {};
  for (const sys of stats?.connected_systems ?? []) {
    usage[sys.id] = { files: sys.file_count, bytes: 0 };
  }
  for (const st of stats?.storage_by_system ?? []) {
    usage[st.system_id] = { files: st.file_count, bytes: st.size_bytes };
  }
  // ถังส่วนตัว (My Drive) มีลิงก์ของตัวเองด้านบนแล้ว — รายการนี้แสดงเฉพาะถังของระบบงาน
  const buckets = systems
    .filter((s) => s.system_type !== "personal")
    .sort((a, b) => a.name.localeCompare(b.name, "th"));

  return (
    <>
    <div className={"side" + (open ? " open" : "")}>
      <div className="ws">
        <span className="ws-logo" style={{ background: "var(--accent)" }}>F</span>
        <div className="ws-name">
          {stats?.workspace_display || "คลังไฟล์กลาง"}
          <div className="t-sm t-muted">{stats?.workspace_name || ""}</div>
        </div>
      </div>

      <GlobalSearch />

      <div style={{ padding: "4px 12px 8px" }}>
        <Link href="/upload" className="btn primary" style={{ width: "100%", justifyContent: "center" }}>
          <Ico.upload className="icon sm" /> อัปโหลดไฟล์
        </Link>
      </div>

      <div className="side-section">
        <SideRow href="/"          icon={<Ico.home />}     label="ภาพรวม" active={nav === "dashboard"} />
        <SideRow href="/files"     icon={<Ico.files />}    label="ไฟล์ทั้งหมด" active={nav === "files"}
                 count={stats ? `${fmtCount(stats.total_files)} ไฟล์` : undefined} />
        <SideRow href="/activity"  icon={<Ico.activity />} label="ความเคลื่อนไหว" active={nav === "activity"} />
        <SideRow href="/share"     icon={<Ico.share />}    label="ลิงก์ที่แชร์" active={nav === "share"} />
        <SideRow href="/archive"   icon={<Ico.archive />}  label="คลังเก็บถาวร" active={nav === "archive"} />
        {/* My Drive — only renders for signed-in users; reads the personal
            drive id from the backend on mount. */}
        <MyDriveLink />
      </div>

      <div className="divider" style={{ margin: "4px 12px" }} />

      <div className="side-section" style={{ flex: 1, overflow: "auto", minHeight: 0 }}>
        <SideLabel action={
          <Link href="/settings#buckets" title="จัดการถังเก็บไฟล์" aria-label="จัดการถังเก็บไฟล์"
                className="t-xs" style={{ color: "var(--accent)" }}>จัดการ</Link>
        }>
          ถังเก็บไฟล์ ({buckets.length})
        </SideLabel>

        {buckets.length === 0 && (
          <div style={{ padding: "8px 10px", fontSize: 12, color: "var(--text-subtle)" }}>
            ยังไม่มีถังเก็บไฟล์
          </div>
        )}

        {buckets.map((sys) => {
          const isActive = sys.id === systemActive;
          const u = usage[sys.id];
          return (
            <React.Fragment key={sys.id}>
              <SideRow
                icon={<span className={"pill " + sys.tone + " sm"} style={{ height: 14, width: 14, padding: 0, justifyContent: "center" }}><span className="dot" /></span>}
                label={<span title={`ถัง ${sys.bucket}${u ? ` · ${fmtCount(u.files)} ไฟล์ · ${fmtBytes(u.bytes)}` : ""}`}>{sys.name}</span>}
                active={isActive}
                count={u ? (u.files > 0 ? `${fmtCount(u.files)} ไฟล์` : "ว่าง") : undefined}
                href={`/files?system_id=${sys.id}`}
              />
              {isActive && orgs.length > 0 && (
                <div className="t-xs t-muted" style={{ padding: "4px 10px 2px 28px" }}>หน่วยงานในถังนี้</div>
              )}
              {isActive && orgs.slice(0, 5).map((o) => (
                <SideRow
                  key={o.id}
                  indent={1}
                  icon={<Ico.users className="icon sm" />}
                  label={o.name}
                  active={orgActive === o.id}
                  href={`/files?system_id=${o.system_id}&org_id=${o.id}`}
                />
              ))}
              {isActive && orgs.length > 5 && (
                <Link href="/orgs" style={{ fontSize: 11, color: "var(--text-subtle)", padding: "4px 10px 4px 28px", display: "block" }}>
                  ดูหน่วยงานทั้งหมด ({orgs.length})
                </Link>
              )}
            </React.Fragment>
          );
        })}

        <details className="side-views" style={{ marginTop: 8 }}>
          <summary className="side-label" style={{ cursor: "pointer", listStyle: "none" }}>
            มุมมองที่บันทึก
          </summary>
          {/* Pulled live from /api/views (pinned rows). */}
          <SavedViewsList />
          <Link href="/views/new" className="t-xs" style={{ display: "block", padding: "4px 10px", color: "var(--accent)" }}>
            + สร้างมุมมองใหม่
          </Link>
        </details>
      </div>

      <div style={{ borderTop: "1px solid var(--border)", padding: "8px 8px" }}>
        <SideRow href="/trash"    icon={<Ico.trash />}    label="ถังขยะ" active={nav === "trash"} />
        <SideRow href="/settings" icon={<Ico.cog />}      label="ตั้งค่า" active={nav === "settings"} />
        <UserMenu />
      </div>
    </div>
    {open && <div className="side-backdrop open" onClick={() => setOpen(false)} aria-hidden="true" />}
    </>
  );
}
