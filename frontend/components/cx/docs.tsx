"use client";

import * as React from "react";
import yaml from "js-yaml";

import { BASE_PATH, type TagColor } from "@/lib/fh";
import { Note, Tag } from "./ui";

type Op = { method: string; path: string; tags?: string[]; summary?: string; description?: string; security?: unknown[]; parameters?: any[]; requestBody?: any; responses?: Record<string, any> };
type Spec = { info: { title: string; version: string; description?: string }; tags?: { name: string; description?: string }[]; paths: Record<string, any>; components?: any };

const METHODS = ["get", "post", "put", "patch", "delete", "head", "options"];
const COLOR: Record<string, TagColor> = { get: "blue", post: "green", put: "orange", patch: "orange", delete: "red" };

/// Small, dependency-free Markdown for the guide text: headings, lists, `code`, **bold**.
function Md({ text }: { text: string }) {
  const inline = (s: string) => s.replace(/\*\*`([^`]+)`\*\*/g, "`$1`").split(/(`[^`]+`|\*\*[^*]+\*\*)/g).map((p, i) =>
    p.startsWith("`") ? <code key={i} className="cx-code">{p.slice(1, -1)}</code> : p.startsWith("**") ? <b key={i}>{p.slice(2, -2)}</b> : p);
  const out: React.ReactNode[] = []; let list: string[] = []; let para: string[] = [];
  const flush = () => {
    if (para.length) { out.push(<p key={out.length} className="cx-doc-p">{inline(para.join(" "))}</p>); para = []; }
    if (list.length) { out.push(<ul key={out.length} className="cx-doc-ul">{list.map((l, i) => <li key={i}>{inline(l)}</li>)}</ul>); list = []; }
  };
  for (const raw of text.split("\n")) {
    const l = raw.trim();
    if (!l) { flush(); continue; }
    if (l.startsWith("## ")) { flush(); out.push(<h3 key={out.length} className="cx-doc-h">{l.slice(3)}</h3>); continue; }
    if (l.startsWith("- ")) { if (para.length) flush(); list.push(l.slice(2)); continue; }
    if (list.length) list[list.length - 1] += " " + l; else para.push(l);
  }
  flush();
  return <>{out}</>;
}

function schemaText(s: any, comp: any, depth = 0): string {
  if (!s) return "";
  if (s.$ref) { const n = s.$ref.split("/").pop(); return depth > 1 ? n : schemaText(comp?.schemas?.[n], comp, depth + 1) || n; }
  if (s.type === "array") return `[${schemaText(s.items, comp, depth + 1)}]`;
  if (s.properties) return `{ ${Object.entries(s.properties).map(([k, v]: any) => `${k}${s.required?.includes(k) ? "*" : ""}: ${v.$ref ? v.$ref.split("/").pop() : v.enum ? v.enum.join("|") : v.type ?? "any"}`).join(", ")} }`;
  return s.type ?? "";
}

export function ApiDocs() {
  const [spec, setSpec] = React.useState<Spec | null>(null);
  const [err, setErr] = React.useState<string | null>(null);
  const [q, setQ] = React.useState("");
  const [open, setOpen] = React.useState<string | null>(null);

  React.useEffect(() => {
    fetch(`${BASE_PATH}/api/openapi.yaml`, { credentials: "include" })
      .then((r) => { if (!r.ok) throw new Error(`HTTP ${r.status}`); return r.text(); })
      .then((t) => setSpec(yaml.load(t) as Spec)).catch((e) => setErr(e.message));
  }, []);

  const ops: Op[] = React.useMemo(() => !spec ? [] : Object.entries(spec.paths).flatMap(([path, item]: any) =>
    METHODS.filter((m) => item[m]).map((m) => ({ ...item[m], parameters: [...(item.parameters ?? []), ...(item[m].parameters ?? [])], method: m, path }))), [spec]);
  const ql = q.trim().toLowerCase();
  const shown = ql ? ops.filter((o) => `${o.method} ${o.path} ${o.summary ?? ""} ${(o.tags ?? []).join(" ")}`.toLowerCase().includes(ql)) : ops;
  const groups = (spec?.tags ?? []).map((t) => ({ ...t, ops: shown.filter((o) => (o.tags ?? ["Other"])[0] === t.name) })).filter((g) => g.ops.length);
  const comp = spec?.components;

  return (
    <div className="cx-page">
      <div className="cx-crumbs">Help</div>
      <div className="cx-title-row"><h1 className="cx-title">API documentation</h1>{spec && <span className="cx-num" style={{ color: "var(--cx-text-helper)" }}>v{spec.info.version} · {ops.length} endpoints</span>}</div>
      {err && <Note kind="error">Could not load the API description ({err}).</Note>}
      {spec && (
        <div className="cx-doc">
          <nav className="cx-doc-toc cx-hide-sm" aria-label="Sections">
            <a href="#guide">Guide</a>
            {groups.map((g) => <a key={g.name} href={`#tag-${g.name}`}>{g.name} <span className="cx-num">{g.ops.length}</span></a>)}
          </nav>
          <div className="cx-doc-main">
            <div className="cx-toolbar" style={{ gap: 8 }}>
              <input className="cx-search" style={{ maxWidth: 360, paddingLeft: 12 }} placeholder="Filter endpoints" aria-label="Filter endpoints" value={q} onChange={(e) => setQ(e.target.value)} />
              <a className="cx-btn cx-btn-ghost" style={{ textDecoration: "none" }} href={`${BASE_PATH}/api/openapi.yaml`} download="filehub-openapi.yaml">Download OpenAPI</a>
            </div>
            {!ql && <section id="guide" className="cx-doc-sec"><Md text={spec.info.description ?? ""} /></section>}
            {groups.map((g) => (
              <section key={g.name} id={`tag-${g.name}`} className="cx-doc-sec">
                <h2 className="cx-doc-h2">{g.name}</h2>
                {g.description && <p className="cx-doc-p" style={{ color: "var(--cx-text-helper)" }}>{g.description}</p>}
                {g.ops.map((o) => {
                  const k = `${o.method} ${o.path}`; const isOpen = open === k;
                  const body = o.requestBody?.content && Object.entries(o.requestBody.content)[0] as [string, any] | undefined;
                  return (
                    <div key={k} className="cx-doc-op" data-open={isOpen || undefined}>
                      <button className="cx-doc-op-h" aria-expanded={isOpen} onClick={() => setOpen(isOpen ? null : k)}>
                        <Tag color={COLOR[o.method] ?? "gray"}>{o.method.toUpperCase()}</Tag>
                        <code className="cx-doc-path">{o.path}</code>
                        <span className="cx-doc-sum">{o.summary}</span>
                        {Array.isArray(o.security) && o.security.length === 0 && <Tag color="gray">Public</Tag>}
                      </button>
                      {isOpen && (
                        <div className="cx-doc-op-b">
                          {o.description && <Md text={o.description} />}
                          {!!o.parameters?.length && (<>
                            <h4 className="cx-doc-h4">Parameters</h4>
                            <div className="cx-table-scroll"><table className="cx-table"><tbody>{o.parameters.map((p: any, i: number) => {
                              const pp = p.$ref ? comp?.parameters?.[p.$ref.split("/").pop()] : p;
                              return <tr key={i}><td className="cx-name"><code>{pp?.name}</code>{pp?.required ? " *" : ""}</td><td>{pp?.in}</td><td>{pp?.schema?.enum?.join(" | ") ?? pp?.schema?.type}</td><td style={{ whiteSpace: "normal" }}>{pp?.description}</td></tr>;
                            })}</tbody></table></div>
                          </>)}
                          {body && (<>
                            <h4 className="cx-doc-h4">Body <span style={{ color: "var(--cx-text-helper)", fontWeight: 400 }}>{body[0]}</span></h4>
                            <pre className="cx-doc-pre">{schemaText(body[1]?.schema, comp)}</pre>
                          </>)}
                          {o.responses && (<>
                            <h4 className="cx-doc-h4">Responses</h4>
                            <div className="cx-table-scroll"><table className="cx-table"><tbody>{Object.entries(o.responses).map(([c, r]: any) => {
                              const rr = r.$ref ? comp?.responses?.[r.$ref.split("/").pop()] : r;
                              return <tr key={c}><td className="cx-num" style={{ width: 64 }}>{c}</td><td style={{ whiteSpace: "normal" }}>{rr?.description}</td></tr>;
                            })}</tbody></table></div>
                          </>)}
                        </div>
                      )}
                    </div>
                  );
                })}
              </section>
            ))}
            {ql && groups.length === 0 && <div className="cx-state">No endpoint matches “{q}”.</div>}
          </div>
        </div>
      )}
    </div>
  );
}
