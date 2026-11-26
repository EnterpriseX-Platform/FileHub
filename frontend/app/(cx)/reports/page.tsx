"use client";
import { Reports } from "@/components/cx/reports";
import { AdminOnly } from "@/components/cx/admin-only";
export default function Page() { return <AdminOnly><Reports /></AdminOnly>; }
