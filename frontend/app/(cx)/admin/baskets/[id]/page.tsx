"use client";
import * as React from "react";
import { useParams, useRouter } from "next/navigation";
/// Basket settings live in the basket page's Settings tab.
export default function Page() {
  const { id } = useParams<{ id: string }>();
  const router = useRouter();
  React.useEffect(() => { router.replace(`/b/${id}?tab=settings`); }, [id, router]);
  return <div className="cx-page"><span className="cx-skel" style={{ width: 200 }} /></div>;
}
