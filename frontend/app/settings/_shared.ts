// Back-compat re-export so existing `import { loadSettingsCtx } from "../_shared"`
// callers keep working.  New code should import `loadServerCtx` directly from
// `@/lib/auth-server`.
export { loadServerCtx as loadSettingsCtx } from "@/lib/auth-server";
