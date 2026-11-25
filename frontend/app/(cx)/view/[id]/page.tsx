"use client";
import { useParams } from "next/navigation";
import { FileViewer } from "@/components/cx/viewer";
export default function Page() { const { id } = useParams<{ id: string }>(); return <FileViewer key={id} id={id} />; }
