// Many people uploading at the same time, spread over FileHub instances
// (FH_BASES=comma list). Each VU loops: one ~200 KB file per request.
import http from "k6/http";
import { check } from "k6";
import { SharedArray } from "k6/data";
import { Counter } from "k6/metrics";

const bases = (__ENV.FH_BASES || "http://127.0.0.1:8090").split(",").map((b) => `${b}/fh/api`);
const sessions = new SharedArray("s", () => JSON.parse(open("./ctx_upload.json")).cookies);
const body = open("./upload-200k.pdf", "b");
const rejected = new Counter("rate_limited");
const SYS = __ENV.SYS || "sys_dms";
const STAGES = (__ENV.STAGES || "200:30s,500:30s,1000:30s,2000:30s").split(",").map((s) => { const [t, d] = s.split(":"); return { target: +t, duration: d }; });

export const options = {
  discardResponseBodies: true,
  scenarios: { burst: { executor: "ramping-vus", startVUs: 0, stages: STAGES, gracefulRampDown: "10s" } },
  thresholds: { http_req_failed: ["rate<0.01"] },
};

export default function () {
  const base = bases[__VU % bases.length];
  const sid = sessions[__VU % sessions.length];
  const r = http.post(`${base}/files`, { system_id: SYS, file: http.file(body, `burst-${__VU}-${__ITER}.pdf`, "application/pdf") },
    { headers: { Cookie: `filehub_session=${sid}` }, timeout: "120s", tags: { kind: "upload" } });
  if (r.status === 429) rejected.add(1);
  check(r, { "uploaded": (x) => x.status === 200 });
}
