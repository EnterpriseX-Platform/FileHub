"use client";
import { useParams } from "next/navigation";
import { BasketSettings } from "@/components/cx/admin";
export default function Page() { const { id } = useParams<{ id: string }>(); return <BasketSettings key={id} id={id} />; }
