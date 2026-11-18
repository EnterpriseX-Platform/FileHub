"use client";
import { useParams } from "next/navigation";
import { VirtualView } from "@/components/cx/vbasket";
export default function Page() { const { id } = useParams<{ id: string }>(); return <VirtualView key={id} id={id} />; }
