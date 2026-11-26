"use client";
import { AdminHome } from "@/components/cx/admin";
import { AdminOnly } from "@/components/cx/admin-only";
export default function Page() { return <AdminOnly><AdminHome /></AdminOnly>; }
