"use client";
import { useParams } from "next/navigation";
import { BasketView } from "@/components/cx/basket-view";
export default function Page() { const { id } = useParams<{ id: string }>(); return <BasketView key={id} basketId={id} />; }
