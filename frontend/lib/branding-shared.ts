/// Per-workspace branding (Settings → General → Branding).  The product ships
/// neutral; an organisation sets its own name, colour, portal link and e-mail
/// hint here instead of in the code.
export type Branding = {
  workspace_display: string;
  org_name: string;
  brand_accent: string;
  login_email_placeholder: string;
  portal_url: string;
  portal_label: string;
  access_help: string;
};

export const DEFAULT_BRANDING: Branding = {
  workspace_display: "File Hub",
  org_name: "",
  brand_accent: "",
  login_email_placeholder: "you@example.com",
  portal_url: "",
  portal_label: "Back to portal",
  access_help: "Ask your administrator to grant you access.",
};

export function withDefaults(b: Partial<Branding> | null | undefined): Branding {
  const out = { ...DEFAULT_BRANDING };
  for (const k of Object.keys(out) as (keyof Branding)[]) {
    const v = b?.[k];
    if (typeof v === "string" && v.trim()) out[k] = v.trim();
  }
  return out;
}

