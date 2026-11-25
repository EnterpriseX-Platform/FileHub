"use client";
import { useParams } from "next/navigation";
import { GoToFolder } from "@/components/cx/lists";
export default function Page() { const { id } = useParams<{ id: string }>(); return <GoToFolder id={id} />; }
