"use client";
import { useSearchParams } from "next/navigation";
import { FileList, searchFilterQuery } from "@/components/cx/lists";
export default function Page() {
  const sp = useSearchParams();
  const q = sp.get("q") ?? "";
  return <FileList kind="search" q={q} filters={searchFilterQuery(new URLSearchParams(sp.toString()))} />;
}
