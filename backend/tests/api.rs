//! Integration tests for every HTTP handler.
//!
//! Each test makes a real HTTP request against the running backend so the
//! routes, query/body parsing, DB layer, encryption, and the multipart
//! decoder are all exercised. The seed data referenced by the UUID
//! constants in `common::*` is what gives these tests known fixtures.

mod common;

use common::*;

use reqwest::{multipart, StatusCode};
use serde_json::Value;
use uuid::Uuid;

// ---- 1. Health ------------------------------------------------------------

#[tokio::test]
async fn health_returns_ok() {
    require_backend().await;
    let r = client().get(format!("{}/api/health", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.text().await.unwrap(), "ok");
}

// `/api/ready` is the k8s readinessProbe target: it must do a real DB +
// storage round-trip and return 200. Regression lock for the `SELECT 1`
// (INT4) vs i64 (INT8) decode mismatch that made it 500 unconditionally —
// which would have stranded every pod in NotReady on the cluster.
#[tokio::test]
async fn ready_returns_ok() {
    require_backend().await;
    let r = client().get(format!("{}/api/ready", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let body: serde_json::Value = r.json().await.unwrap();
    assert_eq!(body["ready"], true);
}

// ---- 2. Dashboard stats --------------------------------------------------

#[tokio::test]
async fn stats_returns_expected_shape() {
    require_backend().await;
    let r: Value = auth_client().await.get(format!("{}/api/stats", base())).send().await.unwrap()
        .json().await.unwrap();
    assert!(r["total_files"].as_i64().unwrap() >= 0);
    assert!(r["total_size_bytes"].as_i64().unwrap() >= 0);
    assert!(r["total_quota_bytes"].as_i64().unwrap() > 0);
    assert!(r["active_orgs"].as_i64().unwrap() >= 1);
    assert_eq!(r["storage_by_system"].as_array().unwrap().len(), 7);
    assert_eq!(r["connected_systems"].as_array().unwrap().len(), 7);
}

// ---- 3. Systems ----------------------------------------------------------

#[tokio::test]
async fn list_systems_seven_with_hr_rose() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await.get(format!("{}/api/systems", base())).send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(r.len(), 7);
    let hr = r.iter().find(|s| s["id"] == SYS_HR).expect("HR system seed missing");
    assert_eq!(hr["tone"], "rose");
    assert_eq!(hr["bucket"], "hr-emp-files");
}

// ---- 4. Orgs -------------------------------------------------------------

#[tokio::test]
async fn list_orgs_filters_by_system() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/orgs?system_id={SYS_HR}", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(r.len() >= 5);
    assert!(r.iter().all(|o| o["system_id"] == SYS_HR));
    assert!(r.iter().any(|o| o["name"].as_str().unwrap().contains("สำนัก พัฒนาฯ")));
}

#[tokio::test]
async fn list_orgs_unknown_system_returns_empty() {
    // system_id is now TEXT (CUID2) — unknown strings just match no rows.
    require_backend().await;
    let r: Vec<Value> = auth_client().await.get(format!("{}/api/orgs?system_id=sys_unknown", base())).send().await.unwrap()
        .json().await.unwrap();
    assert!(r.is_empty());
}

// ---- 5. Files: list + filter --------------------------------------------

#[tokio::test]
async fn list_files_returns_seeded_rows() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await.get(format!("{}/api/files", base())).send().await.unwrap()
        .json().await.unwrap();
    assert!(r.len() >= 12);
    assert!(r.iter().any(|f| f["name"] == "contract-A12.pdf"));
    for i in 0..r.len().saturating_sub(1) {
        assert!(r[i]["modified_at"].as_str() >= r[i + 1]["modified_at"].as_str(),
                "files must be sorted modified_at DESC");
    }
}

#[tokio::test]
async fn list_files_filters_apply() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/files?status=Review", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(!r.is_empty());
    assert!(r.iter().all(|f| f["status"] == "Review"));

    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/files?project=Q1-2026", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(!r.is_empty());
    assert!(r.iter().all(|f| f["project"] == "Q1-2026"));
}

// ---- 6. File detail ------------------------------------------------------

#[tokio::test]
async fn file_detail_returns_metadata() {
    require_backend().await;
    let r: Value = auth_client().await
        .get(format!("{}/api/files/{FILE_001}", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(r["id"], FILE_001);
    assert_eq!(r["name"], "contract-A12.pdf");
    assert_eq!(r["file_type"], "pdf");
    assert_eq!(r["status"], "Review");
}

#[tokio::test]
async fn file_detail_404_for_missing_uuid() {
    require_backend().await;
    let r = auth_client().await
        .get(format!("{}/api/files/{MISSING_UUID}", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn file_detail_400_for_malformed_id() {
    require_backend().await;
    let r = auth_client().await
        .get(format!("{}/api/files/not-a-uuid", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

// ---- 7. Upload → download → delete round trip ---------------------------

async fn upload_text(name: &str, body: &str, system_id: &str) -> Value {
    let c = auth_client().await;
    let part = multipart::Part::text(body.to_string()).file_name(name.to_string());
    let form = multipart::Form::new()
        .part("file", part)
        .text("system_id", system_id.to_string())
        .text("owner", "Test Bot".to_string());
    c.post(format!("{}/api/files", base()))
        .multipart(form)
        .send().await.unwrap()
        .json().await.unwrap()
}

#[tokio::test]
async fn upload_download_delete_roundtrip() {
    require_backend().await;
    let body = "round-trip ทดสอบ ภาษาไทย";
    let row = upload_text("rt-test.txt", body, SYS_HR).await;
    let id: &str = row["id"].as_str().unwrap();

    assert_eq!(row["file_type"], "txt");
    assert_eq!(row["size_bytes"].as_i64().unwrap(), body.as_bytes().len() as i64);
    assert!(row["etag"].as_str().unwrap().len() == 32);

    // Download decrypts to plaintext.
    let dl = auth_client().await.get(format!("{}/api/files/{id}/download", base()))
        .send().await.unwrap().bytes().await.unwrap();
    assert_eq!(&dl[..], body.as_bytes());

    // Delete.
    let del = auth_client().await.delete(format!("{}/api/files/{id}", base()))
        .send().await.unwrap();
    assert_eq!(del.status(), StatusCode::NO_CONTENT);
    let after = auth_client().await.get(format!("{}/api/files/{id}", base()))
        .send().await.unwrap();
    assert_eq!(after.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn upload_without_file_part_returns_400() {
    require_backend().await;
    let form = multipart::Form::new().text("system_id", SYS_HR.to_string());
    let r = auth_client().await.post(format!("{}/api/files", base()))
        .multipart(form)
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn upload_unknown_system_returns_400() {
    require_backend().await;
    let part = multipart::Part::text("x".to_string()).file_name("x.txt".to_string());
    let form = multipart::Form::new()
        .part("file", part)
        .text("system_id", MISSING_UUID.to_string());
    let r = auth_client().await.post(format!("{}/api/files", base()))
        .multipart(form)
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

// ---- 8. Activity ---------------------------------------------------------

#[tokio::test]
async fn activity_returns_seeded_rows() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/activity?limit=20", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(r.len() >= 6);
    for i in 0..r.len().saturating_sub(1) {
        assert!(r[i]["created_at"].as_str() >= r[i + 1]["created_at"].as_str());
    }
}

// ---- 9. Views ------------------------------------------------------------

#[tokio::test]
async fn list_views_returns_seeded_pinned_first() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await.get(format!("{}/api/views", base())).send().await.unwrap()
        .json().await.unwrap();
    assert!(r.len() >= 5);
    assert_eq!(r[0]["pinned"], true);
}

#[tokio::test]
async fn create_view_persists_row() {
    require_backend().await;
    let body = serde_json::json!({
        "name": format!("ITest view {}", Uuid::new_v4()),
        "layout": "table",
        "color": "#16a34a",
        "pinned": false,
    });
    let r: Value = auth_client().await.post(format!("{}/api/views", base()))
        .json(&body)
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(r["layout"], "table");
    assert_eq!(r["color"], "#16a34a");
}

// ---- 10. Permissions -----------------------------------------------------

#[tokio::test]
async fn permissions_for_file_001_have_owner_and_external() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/permissions/{FILE_001}", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(r.len() >= 6);
    assert!(r.iter().any(|p| p["role"] == "owner"));
    assert!(r.iter().any(|p| p["external"] == true));
}

#[tokio::test]
async fn permissions_for_missing_returns_empty_array() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/permissions/{MISSING_UUID}", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(r.len(), 0);
}

// ---- 11. Search (TOR 4.15.14) -------------------------------------------

#[tokio::test]
async fn search_returns_matches_and_respects_filters() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/search?q=contract", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(r.iter().filter(|f| f["name"].as_str().unwrap().contains("contract")).count() >= 2);

    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/search?q=contract&system_id={SYS_HR}", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(r.iter().all(|f| f["system_id"] == SYS_HR));
}

#[tokio::test]
async fn search_empty_q_returns_400() {
    require_backend().await;
    let r = auth_client().await.get(format!("{}/api/search?q=", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

// ---- 12. Versions (TOR 4.15.7) ------------------------------------------

#[tokio::test]
async fn versions_round_trip() {
    require_backend().await;
    let v1 = upload_text("versioned-it.txt", "v1 payload", SYS_HR).await;
    let id  = v1["id"].as_str().unwrap().to_string();

    let part = multipart::Part::text("v2 payload — bigger".to_string())
        .file_name("versioned-it.txt".to_string());
    let form = multipart::Form::new()
        .part("file", part)
        .text("uploaded_by", "Reviewer".to_string())
        .text("note", "rev".to_string());
    let v2: Value = auth_client().await
        .post(format!("{}/api/files/{id}/versions", base()))
        .multipart(form)
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(v2["version"], 2);

    let versions: Vec<Value> = auth_client().await
        .get(format!("{}/api/files/{id}/versions", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(versions.iter().any(|v| v["version"] == 1 && v["uploaded_by"] == "Reviewer"));

    // Clean up.
    let _ = auth_client().await.delete(format!("{}/api/files/{id}", base())).send().await;
}

#[tokio::test]
async fn versions_for_missing_returns_404() {
    require_backend().await;
    let r = auth_client().await
        .get(format!("{}/api/files/{MISSING_UUID}/versions", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

// ---- 13. PATCH (move/rename, TOR 4.15.12) -------------------------------

#[tokio::test]
async fn patch_rename_and_move() {
    require_backend().await;
    let row = upload_text("movable.txt", "move me", SYS_HR).await;
    let id  = row["id"].as_str().unwrap().to_string();

    // Rename.
    let r: Value = auth_client().await.patch(format!("{}/api/files/{id}", base()))
        .json(&serde_json::json!({"name": "renamed.txt"}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["name"], "renamed.txt");

    // Move to Finance system.
    let r: Value = auth_client().await.patch(format!("{}/api/files/{id}", base()))
        .json(&serde_json::json!({"system_id": SYS_FIN, "org_id": ORG_FIN_AP}))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(r["system_id"], SYS_FIN);
    assert_eq!(r["bucket"], "fin-invoices");

    // Download still works after move (and decrypts).
    let body = auth_client().await.get(format!("{}/api/files/{id}/download", base()))
        .send().await.unwrap().bytes().await.unwrap();
    assert_eq!(&body[..], b"move me");

    let _ = auth_client().await.delete(format!("{}/api/files/{id}", base())).send().await;
}

#[tokio::test]
async fn patch_unknown_id_returns_404() {
    require_backend().await;
    let r = auth_client().await.patch(format!("{}/api/files/{MISSING_UUID}", base()))
        .json(&serde_json::json!({"name": "x"}))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn patch_unknown_system_returns_400() {
    require_backend().await;
    let row = upload_text("temp.txt", "x", SYS_HR).await;
    let id = row["id"].as_str().unwrap().to_string();

    let r = auth_client().await.patch(format!("{}/api/files/{id}", base()))
        .json(&serde_json::json!({"system_id": MISSING_UUID}))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);

    let _ = auth_client().await.delete(format!("{}/api/files/{id}", base())).send().await;
}

// ---- 14. Folders (TOR 4.15.4) -------------------------------------------

#[tokio::test]
async fn folder_lifecycle() {
    require_backend().await;
    let body = serde_json::json!({
        "name": format!("ITest folder {}", Uuid::new_v4()),
        "system_id": SYS_HR,
        "color": "#0ea5e9",
        "encrypted": true,
    });
    let f: Value = auth_client().await.post(format!("{}/api/folders", base()))
        .json(&body)
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(f["encrypted"], true);
    let id = f["id"].as_str().unwrap().to_string();

    // List shows it.
    let list: Vec<Value> = auth_client().await
        .get(format!("{}/api/folders?system_id={SYS_HR}", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(list.iter().any(|f| f["id"] == id));

    // Delete.
    let r = auth_client().await.delete(format!("{}/api/folders/{id}", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn folder_create_with_unknown_system_returns_400() {
    require_backend().await;
    let r = auth_client().await.post(format!("{}/api/folders", base()))
        .json(&serde_json::json!({"name": "x", "system_id": MISSING_UUID}))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn folder_delete_missing_returns_404() {
    require_backend().await;
    let r = auth_client().await.delete(format!("{}/api/folders/{MISSING_UUID}", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

// ---- 15. Share links (TOR 4.15.11) --------------------------------------

#[tokio::test]
async fn share_link_round_trip() {
    require_backend().await;
    let row = upload_text("share-it.txt", "share contents", SYS_HR).await;
    let id  = row["id"].as_str().unwrap().to_string();

    let body = serde_json::json!({"created_by": "Anong K.", "expires_in_days": 3});
    let s: Value = auth_client().await.post(format!("{}/api/files/{id}/share", base()))
        .json(&body)
        .send().await.unwrap()
        .json().await.unwrap();
    let token = s["token"].as_str().unwrap().to_string();
    assert!(token.len() >= 16);
    assert!(s["expires_at"].is_string());

    // Anonymous metadata fetch.
    let meta: Value = client().get(format!("{}/api/share/{token}", base()))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(meta["id"], id);

    // Anonymous download decrypts.
    let dl = client().get(format!("{}/api/share/{token}/download", base()))
        .send().await.unwrap().bytes().await.unwrap();
    assert_eq!(&dl[..], b"share contents");

    let _ = auth_client().await.delete(format!("{}/api/files/{id}", base())).send().await;
}

#[tokio::test]
async fn share_unknown_token_returns_404() {
    require_backend().await;
    let r = client().get(format!("{}/api/share/bogus-token-xyz", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

// ---- 16. Reports (TOR 4.15.15) ------------------------------------------

#[tokio::test]
async fn report_by_category_returns_rows() {
    require_backend().await;
    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/reports/by-category", base()))
        .send().await.unwrap()
        .json().await.unwrap();
    assert!(r.len() >= 4);
    for i in 0..r.len().saturating_sub(1) {
        assert!(r[i]["size_bytes"].as_i64().unwrap() >= r[i + 1]["size_bytes"].as_i64().unwrap());
    }
}

#[tokio::test]
async fn report_by_time_accepts_known_buckets() {
    require_backend().await;
    for bucket in ["day", "month", "year"] {
        let r: Vec<Value> = auth_client().await
            .get(format!("{}/api/reports/by-time?bucket={bucket}", base()))
            .send().await.unwrap()
            .json().await.unwrap();
        assert!(!r.is_empty(), "{bucket}");
    }
}

#[tokio::test]
async fn report_by_time_rejects_unknown_bucket() {
    require_backend().await;
    let r = auth_client().await
        .get(format!("{}/api/reports/by-time?bucket=hour", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

// ---- 17. Cache headers (TOR 4.15.16) ------------------------------------

#[tokio::test]
async fn image_download_has_public_cache_control() {
    require_backend().await;
    let part = multipart::Part::bytes(vec![0xff, 0xd8, 0xff, 0xe0]).file_name("photo.png");
    let form = multipart::Form::new()
        .part("file", part)
        .text("system_id", SYS_HR.to_string());
    let row: Value = auth_client().await.post(format!("{}/api/files", base()))
        .multipart(form)
        .send().await.unwrap()
        .json().await.unwrap();
    let id = row["id"].as_str().unwrap().to_string();

    let resp = auth_client().await.get(format!("{}/api/files/{id}/download", base()))
        .send().await.unwrap();
    let cc = resp.headers().get("cache-control").unwrap().to_str().unwrap();
    assert!(cc.contains("public") && cc.contains("max-age=86400"), "got: {cc}");

    let etag = resp.headers().get("etag").expect("etag missing").to_str().unwrap().to_string();
    // 304 round-trip.
    let resp2 = auth_client().await
        .get(format!("{}/api/files/{id}/download", base()))
        .header("If-None-Match", etag)
        .send().await.unwrap();
    assert_eq!(resp2.status(), StatusCode::NOT_MODIFIED);

    let _ = auth_client().await.delete(format!("{}/api/files/{id}", base())).send().await;
}

#[tokio::test]
async fn non_image_download_has_private_cache_control() {
    require_backend().await;
    let row = upload_text("memo.pdf", "%PDF-1.4\n", SYS_HR).await;
    let id  = row["id"].as_str().unwrap().to_string();
    let resp = auth_client().await.get(format!("{}/api/files/{id}/download", base()))
        .send().await.unwrap();
    let cc = resp.headers().get("cache-control").unwrap().to_str().unwrap();
    assert!(cc.contains("private") && cc.contains("no-store"), "got: {cc}");
    let _ = auth_client().await.delete(format!("{}/api/files/{id}", base())).send().await;
}

// ---- 18. Batch upload (TOR 4.15.9) --------------------------------------

#[tokio::test]
async fn batch_upload_creates_three_files() {
    require_backend().await;
    let form = multipart::Form::new()
        .text("system_id", SYS_HR.to_string())
        .text("owner", "Batch Bot".to_string())
        .part("file", multipart::Part::text("a").file_name("a.txt"))
        .part("file", multipart::Part::text("b").file_name("b.pdf"))
        .part("file", multipart::Part::text("c").file_name("c.docx"));
    let r: Vec<Value> = auth_client().await.post(format!("{}/api/files/batch", base()))
        .multipart(form)
        .send().await.unwrap()
        .json().await.unwrap();
    assert_eq!(r.len(), 3);
    assert!(r.iter().all(|f| f["owner"] == "Batch Bot"));

    let types: std::collections::HashSet<&str> = r.iter()
        .map(|f| f["file_type"].as_str().unwrap()).collect();
    assert!(types.contains("txt") && types.contains("pdf") && types.contains("docx"));

    for row in &r {
        let id = row["id"].as_str().unwrap();
        let _ = auth_client().await.delete(format!("{}/api/files/{id}", base())).send().await;
    }
}

#[tokio::test]
async fn batch_upload_without_files_returns_400() {
    require_backend().await;
    let form = multipart::Form::new().text("system_id", SYS_HR.to_string());
    let r = auth_client().await.post(format!("{}/api/files/batch", base()))
        .multipart(form)
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
}

// ---- 18b. TUS resumable upload — regression tests for the May 2026 fixes
//
// Three bugs hid in the TUS path that all rendered as "the upload bar sits at
// 0 % forever" client-side; this section locks each in so they can't regress:
//   1. `create_session` previously returned `Location: /fh/api/uploads/<id>`,
//      an absolute path the Next.js proxy doesn't recognise.  We now return a
//      relative `uploads/<id>` that resolves correctly under any prefix.
//   2. Axum's default `Bytes` body limit (2 MiB) made any PATCH chunk above
//      that reset the connection.  TUS routes now carry a 64 MiB
//      `DefaultBodyLimit::max(...)` layer.
//   3. tus-js-client encodes empty-string metadata values verbatim; binding
//      `org_id=""` into `tus_uploads` tripped the FK constraint.  Empty
//      strings are now coerced to `NULL` before the insert.
// -----------------------------------------------------------------------------

const TUS_RESUMABLE: &str = "1.0.0";

fn b64(s: &str) -> String {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    B64.encode(s)
}

/// Bug #1 — Location header MUST be relative so the proxy-rewritten URL
/// resolves back to the same `/api/uploads/<id>` the browser hit.
#[tokio::test]
async fn tus_creation_returns_relative_location() {
    require_backend().await;
    let r = auth_client().await
        .post(format!("{}/api/uploads", base()))
        .header("tus-resumable",  TUS_RESUMABLE)
        .header("upload-length",  "1024")
        .header("upload-metadata",
            format!("filename {},system_id {},owner {}",
                b64("loc-fmt.bin"), b64(SYS_HR), b64("Anong K.")))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::CREATED);
    let loc = r.headers().get("location").expect("Location header")
        .to_str().unwrap().to_string();
    assert!(
        loc.starts_with("uploads/"),
        "Location must be relative `uploads/<id>`; got `{loc}`. \
         An absolute path here breaks tus-js-client URL resolution behind the Next.js proxy.",
    );
    // Cleanup
    let id = loc.trim_start_matches("uploads/");
    let _ = auth_client().await
        .delete(format!("{}/api/uploads/{}", base(), id))
        .header("tus-resumable", TUS_RESUMABLE)
        .send().await;
}

/// Bug #3 — empty-string metadata values must not be bound into FK columns.
/// Pre-fix this returned 500 with `tus_uploads_org_id_fkey` from Postgres.
#[tokio::test]
async fn tus_creation_treats_empty_metadata_as_absent() {
    require_backend().await;
    // Mirror what tus-js-client sends when no org / project / folder is picked:
    // every key is present, several with empty string values.
    let meta = [
        format!("filename {}",     b64("empty-meta.bin")),
        format!("content_type {}", b64("application/octet-stream")),
        format!("system_id {}",    b64(SYS_HR)),
        format!("org_id {}",       b64("")),     // ← was the FK trip
        format!("folder_id {}",    b64("")),
        format!("project {}",      b64("")),
        format!("status {}",       b64("Draft")),
        format!("owner {}",        b64("Anong K.")),
        format!("tags {}",         b64("[]")),
    ].join(",");
    let r = auth_client().await
        .post(format!("{}/api/uploads", base()))
        .header("tus-resumable", TUS_RESUMABLE)
        .header("upload-length", "8")
        .header("upload-metadata", meta)
        .send().await.unwrap();
    assert_eq!(
        r.status(), StatusCode::CREATED,
        "Empty-string metadata fields must be coerced to NULL, not bound verbatim. \
         Got {} — likely an FK violation on org_id or folder_id resurfaced.",
        r.status(),
    );
    let id = r.headers().get("location").unwrap()
        .to_str().unwrap().trim_start_matches("uploads/").to_string();
    let _ = auth_client().await
        .delete(format!("{}/api/uploads/{}", base(), id))
        .header("tus-resumable", TUS_RESUMABLE)
        .send().await;
}

/// Bug #2 + #1 + happy path — PATCH must accept a chunk well above Axum's
/// 2 MiB default Bytes-extractor limit, the HEAD progress endpoint must
/// reflect the new offset, and finalising the upload must insert a `files`
/// row that lists in `/api/files`.
#[tokio::test]
async fn tus_full_roundtrip_with_large_chunk() {
    require_backend().await;
    const SIZE: usize = 4 * 1024 * 1024; // 4 MiB — comfortably past the 2 MiB default
    let client = auth_client().await;

    // 1. CREATE
    let unique = format!("tus-roundtrip-{}.bin", &Uuid::now_v7().to_string()[..8]);
    let create = client
        .post(format!("{}/api/uploads", base()))
        .header("tus-resumable", TUS_RESUMABLE)
        .header("upload-length", SIZE.to_string())
        .header("upload-metadata",
            format!("filename {},system_id {},owner {},status {}",
                b64(&unique), b64(SYS_HR), b64("Anong K."), b64("Draft")))
        .send().await.unwrap();
    assert_eq!(create.status(), StatusCode::CREATED);
    let loc = create.headers().get("location").unwrap().to_str().unwrap().to_string();
    let id = loc.trim_start_matches("uploads/");
    let session_url = format!("{}/api/uploads/{}", base(), id);

    // 2. PATCH the whole body in one shot. Pre-fix this hit DefaultBodyLimit.
    let payload = vec![0xABu8; SIZE];
    let patch = client
        .patch(&session_url)
        .header("tus-resumable",  TUS_RESUMABLE)
        .header("upload-offset",  "0")
        .header("content-type",   "application/offset+octet-stream")
        .body(payload)
        .send().await.unwrap();
    assert_eq!(
        patch.status(), StatusCode::NO_CONTENT,
        "PATCH must accept a 4 MiB chunk; if this comes back 413/500 the body limit fix regressed",
    );
    assert_eq!(
        patch.headers().get("upload-offset").unwrap().to_str().unwrap(),
        SIZE.to_string(),
        "Upload-Offset must reflect the full body after the final chunk",
    );

    // 3. HEAD progress endpoint also reports completion (sanity)
    let head = client
        .request(reqwest::Method::HEAD, &session_url)
        .header("tus-resumable", TUS_RESUMABLE)
        .send().await.unwrap();
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers().get("upload-offset").unwrap().to_str().unwrap(), SIZE.to_string());
    assert_eq!(head.headers().get("upload-length").unwrap().to_str().unwrap(), SIZE.to_string());

    // 4. The `finalise` step must have written a `files` row.  Look for it
    //    via the regular listing so we exercise the same query the UI uses.
    let listed: Vec<Value> = client
        .get(format!("{}/api/files?system_id={}&limit=20", base(), SYS_HR))
        .send().await.unwrap().json().await.unwrap();
    let row = listed.iter().find(|f| f["name"] == unique)
        .unwrap_or_else(|| panic!("TUS-finalised file `{unique}` not present in HR listing — finalise() didn't run"));
    assert_eq!(row["size_bytes"].as_i64().unwrap() as usize, SIZE);
    assert_eq!(row["status"], "Draft");
    assert_eq!(row["owner"], "Anong K.");

    // Cleanup the file row + bytes so repeated runs stay deterministic.
    let file_id = row["id"].as_str().unwrap();
    let _ = client.delete(format!("{}/api/files/{}", base(), file_id)).send().await;
}

// ---- 19. UUID strictness ------------------------------------------------

#[tokio::test]
async fn all_seeded_keys_match_their_type() {
    // Hybrid keys: systems/orgs/folders/views/users use CUID2 (TEXT), files +
    // other transaction tables use UUIDs.  Audit both sides.
    require_backend().await;
    let systems: Vec<Value> = auth_client().await.get(format!("{}/api/systems", base())).send().await.unwrap()
        .json().await.unwrap();
    for s in systems {
        let id = s["id"].as_str().unwrap();
        assert!(id.starts_with("sys_"), "system id should be cuid-style: {id}");
    }
    let files: Vec<Value> = auth_client().await.get(format!("{}/api/files", base())).send().await.unwrap()
        .json().await.unwrap();
    for f in files {
        Uuid::parse_str(f["id"].as_str().unwrap()).expect("file id should be uuid");
    }
    let orgs: Vec<Value> = auth_client().await.get(format!("{}/api/orgs", base())).send().await.unwrap()
        .json().await.unwrap();
    for o in orgs {
        let id = o["id"].as_str().unwrap();
        assert!(id.starts_with("org_"), "org id should be cuid-style: {id}");
    }
}

// ---- 19. Check-out / check-in locking (TOR 5.3.8.4-5) -------------------

/// Full happy path for a single editor: a fresh file starts unlocked, can be
/// checked out (the caller becomes the holder), and checked back in.
#[tokio::test]
async fn checkout_lifecycle() {
    require_backend().await;
    let row = upload_text("checkout-life.txt", "lock me", SYS_HR).await;
    let id = row["id"].as_str().unwrap().to_string();
    let c = auth_client().await;

    let lock: Value = c.get(format!("{}/api/files/{id}/lock", base()))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(lock["locked"], false);

    let out: Value = c.post(format!("{}/api/files/{id}/checkout", base()))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(out["locked"], true);
    assert_eq!(out["by_me"], true);
    assert!(out["by_name"].as_str().is_some());

    // Checking out again as the same holder is idempotent, not a conflict.
    let again = c.post(format!("{}/api/files/{id}/checkout", base())).send().await.unwrap();
    assert_eq!(again.status(), StatusCode::OK);

    let back: Value = c.post(format!("{}/api/files/{id}/checkin", base()))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(back["locked"], false);

    let _ = c.delete(format!("{}/api/files/{id}", base())).send().await;
}

/// A lock held by one user blocks both a new-version upload and a checkout
/// attempt by a *different* non-admin user (409), and clears once released.
#[tokio::test]
async fn checkout_blocks_other_users() {
    require_backend().await;
    // anong (editor) owns the file; admin takes the lock.
    let row = upload_text("checkout-block.txt", "v1", SYS_HR).await;
    let id = row["id"].as_str().unwrap().to_string();
    let admin = auth_client_as("admin@acme.go.th", "admin123").await;
    let editor = auth_client().await; // anong, non-holder, non-admin

    let out: Value = admin.post(format!("{}/api/files/{id}/checkout", base()))
        .send().await.unwrap().json().await.unwrap();
    assert_eq!(out["locked"], true);

    // The non-holder editor can't upload a new version…
    let part = multipart::Part::text("v2".to_string()).file_name("checkout-block.txt".to_string());
    let form = multipart::Form::new().part("file", part);
    let blocked = editor.post(format!("{}/api/files/{id}/versions", base()))
        .multipart(form).send().await.unwrap();
    assert_eq!(blocked.status(), StatusCode::CONFLICT);

    // …nor steal the lock.
    let steal = editor.post(format!("{}/api/files/{id}/checkout", base())).send().await.unwrap();
    assert_eq!(steal.status(), StatusCode::CONFLICT);

    // Admin releases; the editor can now take it.
    let ci = admin.post(format!("{}/api/files/{id}/checkin", base())).send().await.unwrap();
    assert_eq!(ci.status(), StatusCode::OK);
    let taken = editor.post(format!("{}/api/files/{id}/checkout", base())).send().await.unwrap();
    assert_eq!(taken.status(), StatusCode::OK);

    let _ = editor.delete(format!("{}/api/files/{id}", base())).send().await;
}

/// Viewers are read-only: checkout is gated by require_role before anything
/// else, so even a valid file id returns 403.
#[tokio::test]
async fn checkout_forbidden_for_viewer() {
    require_backend().await;
    let viewer = auth_client_as("viewer@acme.go.th", "viewer123").await;
    let r = viewer.post(format!("{}/api/files/{FILE_001}/checkout", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

// ---- 20. Status report + CSV exports (TOR 5.3.1.16, 5.3.7.6) ------------

#[tokio::test]
async fn report_status_returns_counts_and_items() {
    require_backend().await;
    let rep: Value = auth_client().await
        .get(format!("{}/api/reports/status", base()))
        .send().await.unwrap().json().await.unwrap();
    assert!(rep["active"].as_i64().unwrap() >= 1);
    for k in ["inactive", "retention", "deleted"] {
        assert!(rep[k].as_i64().is_some(), "missing count {k}");
    }
    let items = rep["items"].as_array().unwrap();
    for it in items {
        let state = it["state"].as_str().unwrap();
        assert!(
            matches!(state, "active" | "inactive" | "retention"),
            "unexpected lifecycle state: {state}"
        );
    }
}

#[tokio::test]
async fn report_status_csv_has_bom_and_header() {
    require_backend().await;
    let r = auth_client().await
        .get(format!("{}/api/reports/status.csv", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers()["content-type"].to_str().unwrap().starts_with("text/csv"));
    let body = r.text().await.unwrap();
    // UTF-8 BOM so Excel renders Thai; header row follows immediately.
    assert!(body.starts_with('\u{feff}'), "CSV should lead with a UTF-8 BOM");
    assert!(body.trim_start_matches('\u{feff}').starts_with("name,system,status,state,"));
}

#[tokio::test]
async fn audit_export_csv_filters_and_streams() {
    require_backend().await;
    let r = auth_client().await
        .get(format!("{}/api/activity/export.csv?user=anong", base()))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert!(r.headers()["content-type"].to_str().unwrap().starts_with("text/csv"));
    let body = r.text().await.unwrap();
    assert!(body.starts_with('\u{feff}'));
    assert!(body.trim_start_matches('\u{feff}').starts_with("timestamp,user,action,target,system"));
}

#[tokio::test]
async fn ai_usage_report_is_admin_only() {
    require_backend().await;
    // Admin gets the report (shape holds even with zero usage rows).
    let admin = auth_client_as("admin@acme.go.th", "admin123").await;
    let r = admin.get(format!("{}/api/reports/ai-usage", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let v: Value = r.json().await.unwrap();
    assert!(v["total_ops"].as_i64().is_some());
    assert!(v["by_op"].is_array());
    assert!(v["by_model"].is_array());

    // Editor and viewer are billing-blind — 403.
    for (email, pw) in [("anong@acme.go.th", "anong123"), ("viewer@acme.go.th", "viewer123")] {
        let c = auth_client_as(email, pw).await;
        let r = c.get(format!("{}/api/reports/ai-usage", base())).send().await.unwrap();
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{email} should be forbidden");
    }
}

// ---- 21. Starred / favorites ------------------------------------------------

#[tokio::test]
async fn star_lifecycle() {
    require_backend().await;
    let row = upload_text("star-me.txt", "fav", SYS_HR).await;
    let id = row["id"].as_str().unwrap().to_string();
    let c = auth_client().await;

    // Initially not starred.
    let s0: Value = c.get(format!("{}/api/files/{id}/star", base())).send().await.unwrap().json().await.unwrap();
    assert_eq!(s0["starred"], false);

    // Star (idempotent — do it twice).
    let s1: Value = c.put(format!("{}/api/files/{id}/star", base())).send().await.unwrap().json().await.unwrap();
    assert_eq!(s1["starred"], true);
    let _ = c.put(format!("{}/api/files/{id}/star", base())).send().await.unwrap();

    // Appears in the starred list.
    let list: Vec<Value> = c.get(format!("{}/api/starred", base())).send().await.unwrap().json().await.unwrap();
    assert!(list.iter().any(|f| f["id"] == id), "starred list should contain the file");

    // Unstar → gone.
    let s2: Value = c.delete(format!("{}/api/files/{id}/star", base())).send().await.unwrap().json().await.unwrap();
    assert_eq!(s2["starred"], false);
    let list2: Vec<Value> = c.get(format!("{}/api/starred", base())).send().await.unwrap().json().await.unwrap();
    assert!(!list2.iter().any(|f| f["id"] == id));

    let _ = c.delete(format!("{}/api/files/{id}", base())).send().await;
}

#[tokio::test]
async fn star_missing_file_returns_404() {
    require_backend().await;
    let r = auth_client().await.put(format!("{}/api/files/{MISSING_UUID}/star", base())).send().await.unwrap();
    assert_eq!(r.status(), StatusCode::NOT_FOUND);
}

// ---- 22. Electronic signatures ---------------------------------------------

#[tokio::test]
async fn esign_signature_library_crud() {
    require_backend().await;
    let c = auth_client().await;
    let created: Value = c.post(format!("{}/api/signatures", base()))
        .json(&serde_json::json!({"label":"My sig","kind":"typed","image":"data:image/png;base64,AAAA"}))
        .send().await.unwrap().json().await.unwrap();
    let sid = created["id"].as_str().unwrap().to_string();
    let list: Vec<Value> = c.get(format!("{}/api/signatures", base())).send().await.unwrap().json().await.unwrap();
    assert!(list.iter().any(|s| s["id"] == sid));
    let del = c.delete(format!("{}/api/signatures/{sid}", base())).send().await.unwrap();
    assert_eq!(del.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn esign_single_signer_flow() {
    require_backend().await;
    let row = upload_text("sign-me.txt", "please sign", SYS_HR).await;
    let id = row["id"].as_str().unwrap().to_string();
    let c = auth_client().await; // anong = usr_anong

    let req: Value = c.post(format!("{}/api/files/{id}/sign-requests", base()))
        .json(&serde_json::json!({"signers":[{"user_id":"usr_anong"}]}))
        .send().await.unwrap().json().await.unwrap();
    let req_id = req["id"].as_str().unwrap().to_string();
    assert_eq!(req["status"], "pending");

    // Appears in my queue as my turn.
    let q: Vec<Value> = c.get(format!("{}/api/sign-requests/mine", base())).send().await.unwrap().json().await.unwrap();
    assert!(q.iter().any(|x| x["request_id"] == req_id && x["my_turn"] == true));

    // Sign → request completes.
    let signed: Value = c.post(format!("{}/api/sign-requests/{req_id}/sign", base()))
        .json(&serde_json::json!({})).send().await.unwrap().json().await.unwrap();
    assert_eq!(signed["status"], "completed");

    // Verify → intact (document unchanged since signing).
    let v: Value = c.get(format!("{}/api/sign-requests/{req_id}/verify", base())).send().await.unwrap().json().await.unwrap();
    assert_eq!(v["intact"], true);
    assert_eq!(v["status"], "completed");

    let _ = c.delete(format!("{}/api/files/{id}", base())).send().await;
}

#[tokio::test]
async fn esign_sequential_turn_guard() {
    require_backend().await;
    let row = upload_text("sign-seq.txt", "x", SYS_HR).await;
    let id = row["id"].as_str().unwrap().to_string();
    let anong = auth_client().await;

    // admin signs first (seq 0), anong second (seq 1).
    let req: Value = anong.post(format!("{}/api/files/{id}/sign-requests", base()))
        .json(&serde_json::json!({
            "order_mode":"sequential",
            "signers":[{"user_id":"usr_admin","seq":0},{"user_id":"usr_anong","seq":1}]
        }))
        .send().await.unwrap().json().await.unwrap();
    let req_id = req["id"].as_str().unwrap().to_string();

    // anong (seq 1) can't sign before admin (seq 0) → 409.
    let early = anong.post(format!("{}/api/sign-requests/{req_id}/sign", base()))
        .json(&serde_json::json!({})).send().await.unwrap();
    assert_eq!(early.status(), StatusCode::CONFLICT);

    let _ = anong.delete(format!("{}/api/files/{id}", base())).send().await;
}

// ---- 23. Boolean search + version restore ----------------------------------

#[tokio::test]
async fn search_boolean_or() {
    require_backend().await;
    // Distinct bare-word content so the full-text tokens are unambiguous.
    let a = upload_text("bool-a.txt", "alphaword shared", SYS_HR).await;
    let b = upload_text("bool-b.txt", "deltaword shared", SYS_HR).await;
    let ida = a["id"].as_str().unwrap().to_string();
    let idb = b["id"].as_str().unwrap().to_string();

    // OR matches either file.
    let r: Vec<Value> = auth_client().await
        .get(format!("{}/api/search?q=alphaword%20OR%20deltaword", base()))
        .send().await.unwrap().json().await.unwrap();
    let ids: Vec<&str> = r.iter().filter_map(|f| f["id"].as_str()).collect();
    assert!(ids.contains(&ida.as_str()) && ids.contains(&idb.as_str()), "OR should match both files");

    // AND (space) of two words that never co-occur matches neither.
    let r2: Vec<Value> = auth_client().await
        .get(format!("{}/api/search?q=alphaword%20deltaword", base()))
        .send().await.unwrap().json().await.unwrap();
    let ids2: Vec<&str> = r2.iter().filter_map(|f| f["id"].as_str()).collect();
    assert!(!ids2.contains(&ida.as_str()) && !ids2.contains(&idb.as_str()), "AND of non-co-occurring words matches neither");

    let c = auth_client().await;
    let _ = c.delete(format!("{}/api/files/{ida}", base())).send().await;
    let _ = c.delete(format!("{}/api/files/{idb}", base())).send().await;
}

#[tokio::test]
async fn restore_version_roundtrip() {
    require_backend().await;
    let v1 = upload_text("restore-it.txt", "aaa-original", SYS_HR).await;
    let id = v1["id"].as_str().unwrap().to_string();

    // Upload v2 (new bytes).
    let part = multipart::Part::text("bbb-updated".to_string()).file_name("restore-it.txt".to_string());
    let form = multipart::Form::new().part("file", part);
    let v2: Value = auth_client().await.post(format!("{}/api/files/{id}/versions", base()))
        .multipart(form).send().await.unwrap().json().await.unwrap();
    assert_eq!(v2["version"], 2);

    // Restore version 1 → current bytes revert to the original.
    let restored: Value = auth_client().await
        .post(format!("{}/api/files/{id}/versions/1/restore", base()))
        .send().await.unwrap().json().await.unwrap();
    assert!(restored["version"].as_i64().unwrap() >= 3, "version should bump past current");

    let dl = auth_client().await.get(format!("{}/api/files/{id}/download", base()))
        .send().await.unwrap().bytes().await.unwrap();
    assert_eq!(&dl[..], b"aaa-original", "download should return the restored v1 bytes");

    let _ = auth_client().await.delete(format!("{}/api/files/{id}", base())).send().await;
}

#[tokio::test]
async fn esign_create_requires_editor() {
    require_backend().await;
    let viewer = auth_client_as("viewer@acme.go.th", "viewer123").await;
    let r = viewer.post(format!("{}/api/files/{FILE_001}/sign-requests", base()))
        .json(&serde_json::json!({"signers":[{"user_id":"usr_viewer"}]}))
        .send().await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}
