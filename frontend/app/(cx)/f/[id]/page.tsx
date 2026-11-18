"use client";
import { useParams } from "next/navigation";
import { GoToFile } from "@/components/cx/lists";
export default function Page() { const { id } = useParams<{ id: string }>(); return <GoToFile id={id} />; }
