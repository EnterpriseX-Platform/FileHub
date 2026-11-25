import { redirect } from "next/navigation";

/// The workspace opens on Recent; the older dashboard lives at /overview.
export default function Home() { redirect("/recent"); }
