// FileHub load test (local). Sessions come from prep.py (one per load user).
import http from "k6/http";
import { check } from "k6";
import { SharedArray } from "k6/data";

const B = "http://127.0.0.1:8090/fh/api";
const ctx = JSON.parse(open("./ctx.json"));
const sessions = new SharedArray("s", () => ctx.cookies);
const pdf = open("./small.pdf", "b");

const DUR = __ENV.DUR || "45s";
export const options = {
  discardResponseBodies: true,
  scenarios: {
    browse:   { executor: "constant-vus", vus: 40, duration: DUR, exec: "browse" },
    search:   { executor: "constant-vus", vus: 10, duration: DUR, exec: "search" },
    vtree:    { executor: "constant-vus", vus: 10, duration: DUR, exec: "vtree" },
    download: { executor: "constant-vus", vus: 20, duration: DUR, exec: "download" },
    upload:   { executor: "constant-arrival-rate", rate: 15, timeUnit: "1s", duration: DUR, preAllocatedVUs: 20, exec: "upload" },
    s3get:    { executor: "constant-vus", vus: 10, duration: DUR, exec: "s3get" },
  },
  thresholds: {
    "http_req_failed": ["rate<0.01"],
    "http_req_duration{scenario:browse}": ["p(95)<800"],
    "http_req_duration{scenario:search}": ["p(95)<1500"],
    "http_req_duration{scenario:vtree}": ["p(95)<1500"],
    "http_req_duration{scenario:download}": ["p(95)<500"],
    "http_req_duration{scenario:upload}": ["p(95)<1500"],
    "http_req_duration{scenario:s3get}": ["p(95)<500"],
  },
};

const hdr = () => ({ headers: { Cookie: `filehub_session=${sessions[(__VU + __ITER) % sessions.length]}` } });

export function browse() {
  const sys = ["sys_fin", "sys_dms", "sys_hr"][__ITER % 3];
  const r = http.get(`${B}/files?system_id=${sys}&folder_id=null&sort=modified&dir=desc&limit=50`, hdr());
  check(r, { "list 200": (x) => x.status === 200 });
}
export function search() {
  const r = http.get(`${B}/search?q=load-${1000 + (__ITER % 9000)}&limit=50`, hdr());
  check(r, { "search 200": (x) => x.status === 200 });
}
export function vtree() {
  const r = http.get(`${B}/vbaskets/${ctx.vbasket}/tree`, hdr());
  check(r, { "vtree 200": (x) => x.status === 200 });
}
export function download() {
  const r = http.get(`${B}/files/${ctx.files[__ITER % ctx.files.length]}/download`, hdr());
  check(r, { "download 200": (x) => x.status === 200 });
}
export function upload() {
  const r = http.post(`${B}/files`, { file: http.file(pdf, `load-up-${__VU}-${__ITER}.pdf`, "application/pdf"), system_id: "sys_dms" }, hdr());
  check(r, { "upload 200": (x) => x.status === 200 });
}
export function s3get() {
  const r = http.get(ctx.presigned[__ITER % ctx.presigned.length]);
  check(r, { "s3 200": (x) => x.status === 200 });
}
