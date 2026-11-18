import http from "k6/http";
const ctx = JSON.parse(open("./ctx.json"));
const B = "http://127.0.0.1:8090/fh/api";
export const options = { discardResponseBodies: true, vus: 20, duration: "15s" };
export default function () {
  const c = { headers: { Cookie: `filehub_session=${ctx.cookies[__VU % ctx.cookies.length]}` } };
  if (__ENV.T === "health") http.get(`${B}/health`);
  if (__ENV.T === "me") http.get(`${B}/auth/me`, c);
  if (__ENV.T === "download") http.get(`${B}/files/${ctx.files[__ITER % 20]}/download`, c);
  if (__ENV.T === "s3") http.get(ctx.presigned[__ITER % 10]);
  if (__ENV.T === "list") http.get(`${B}/files?system_id=sys_fin&folder_id=null&limit=50`, c);
}
