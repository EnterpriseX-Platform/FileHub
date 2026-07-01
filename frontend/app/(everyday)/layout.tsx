import { EverydayShell } from "@/components/everyday/shell";

/// Layout for the Everyday (consumer) persona. The route group `(everyday)` is
/// URL-transparent, so its pages live at /home, /my, etc. The shell (top bar +
/// nav) renders once here around all everyday screens.
export default function EverydayLayout({ children }: { children: React.ReactNode }) {
  return <EverydayShell>{children}</EverydayShell>;
}
