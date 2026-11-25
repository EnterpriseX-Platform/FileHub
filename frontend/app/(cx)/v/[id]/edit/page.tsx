"use client";
import { useParams } from "next/navigation";
import { RulesEditor } from "@/components/cx/vbasket";
export default function Page() { const { id } = useParams<{ id: string }>(); return <RulesEditor key={id} id={id} />; }
