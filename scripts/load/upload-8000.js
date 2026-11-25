// 8,000 people press Upload at the same moment, each sends one ~200 KB file.
// Measures: does every upload succeed, and how long until the last one is done.
import http from "k6/http";
import { check } from "k6";
import { SharedArray } from "k6/data";

const bases = (__ENV.FH_BASES || "http://127.0.0.1:8090").split(",").map((b) => `${b}/fh/api`);
const sessions = new SharedArray("s", () => JSON.parse(open("./ctx_upload.json")).cookies);
const body = open("./upload-200k.pdf", "b");
const N = +(__ENV.N || 8000);
const RAMP = +(__ENV.RAMP || 5);

export const options = {
  discardResponseBodies: true,
  // N people each press Upload once, all within RAMP seconds (default 5).
  scenarios: { all_at_once: { executor: "constant-arrival-rate", rate: Math.ceil(N / RAMP), timeUnit: "1s",
    duration: `${RAMP}s`, preAllocatedVUs: N, maxVUs: N, gracefulStop: "10m" } },
  thresholds: { http_req_failed: ["rate<0.001"] },
};

export default function () {
  const r = http.post(`${bases[__VU % bases.length]}/files`,
    { system_id: __ENV.SYS || "sys_dms", file: http.file(body, `at-once-${__VU}.pdf`, "application/pdf") },
    { headers: { Cookie: `filehub_session=${sessions[__VU % sessions.length]}` }, timeout: "600s" });
  check(r, { uploaded: (x) => x.status === 200 });
}
