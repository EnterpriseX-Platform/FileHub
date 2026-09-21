//! นำเข้าไฟล์จากฮับเดิมเข้ามาที่ FileHub ตัวใหม่ (NEB — 22 ก.ย. 2569)
//!
//! ## ทำไมต้องมี
//! ฮับเดิมของ NEB มีไฟล์ราว 6 หมื่นใบ (PROD 62,900 ใบ / 41.9 GB) ที่ระบบงานต่าง ๆ
//! อ้างถึงด้วย `FILE_ID` ที่เก็บไว้ใน Oracle ของตัวเอง ถ้าย้ายแล้ว "รหัสเปลี่ยน"
//! ก็ต้องไล่แก้ข้อมูลในฐานของทุกโมดูล ซึ่งเป็นไปไม่ได้ในทางปฏิบัติ
//!
//! 🔑 ตัวนี้จึง **คงรหัสไฟล์เดิมไว้ทุกใบ** (ทั้งสองฝั่งเป็น UUID เหมือนกัน)
//! ⇒ ไฟล์ที่ย้ายมาแล้วเปิดด้วยรหัสเดิมได้ทันที ไม่ต้องแตะข้อมูลของโมดูลใดเลย
//! และเมื่อย้ายครบก็ปลดฮับเดิมได้จริง (ไม่ต้องพึ่งการส่งต่อคำขออีก)
//!
//! ## ทำซ้ำได้
//! ใบไหนมีอยู่แล้วจะข้าม (นับเป็น skipped) ⇒ รันซ้ำเพื่อไล่เก็บส่วนที่เหลือได้
//! และรันระหว่างระบบเปิดใช้งานได้ เพราะไม่แตะของเดิมเลย (อ่านอย่างเดียว)
//!
//! ## ตั้งใจไม่ทำ thumbnail / สกัดข้อความตอนนำเข้า
//! 6 หมื่นใบจะถล่มเครื่องทันที — ให้ทยอยทำทีหลังเป็นงานเบื้องหลังแยกต่างหาก

use std::sync::Arc;

use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::handlers::sanitize_filename;
use crate::models::System;
use crate::AppState;

#[derive(Deserialize)]
pub struct ImportRequest {
    /// จำนวนใบที่จะไล่ดูต่อรอบ (ค่าเริ่มต้น 50 · สูงสุด 500)
    pub limit: Option<i64>,
    /// ตำแหน่งเริ่มของรายการฝั่งฮับเดิม
    pub offset: Option<i64>,
    /// ระบุรหัสไฟล์เองก็ได้ (ใช้ตอนไล่เก็บใบที่ตกหล่น)
    pub ids: Option<Vec<String>>,
    /// ดูเฉย ๆ ไม่เขียนจริง
    pub dry_run: Option<bool>,
}

#[derive(Serialize, Default)]
pub struct ImportResult {
    pub scanned: usize,
    pub imported: usize,
    pub skipped_existing: usize,
    pub failed: usize,
    pub bytes: i64,
    pub errors: Vec<String>,
    pub dry_run: bool,
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

async fn legacy_token(client: &reqwest::Client) -> Option<String> {
    let url = env_opt("LEGACY_TOKEN_URL")?;
    let v: serde_json::Value = client.get(&url).send().await.ok()?.json().await.ok()?;
    v.get("token").and_then(|t| t.as_str()).map(|s| s.to_string())
}

fn s(v: &serde_json::Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(|x| x.trim().to_string()).filter(|x| !x.is_empty())
}

/// นำเข้าไฟล์จากฮับเดิม — admin เท่านั้น
pub async fn import_from_legacy(
    State(st): State<Arc<AppState>>,
    user: AuthUser,
    Json(req): Json<ImportRequest>,
) -> ApiResult<Json<ImportResult>> {
    crate::auth::require_role(&user.0, &["admin"])?;
    let base = env_opt("LEGACY_FILEHUB_URL")
        .ok_or_else(|| ApiError::BadRequest("ยังไม่ได้ตั้ง LEGACY_FILEHUB_URL".into()))?;
    let system_id = env_opt("LEGACY_DEFAULT_SYSTEM")
        .or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"))
        .ok_or_else(|| ApiError::BadRequest("ยังไม่ได้ตั้ง LEGACY_DEFAULT_SYSTEM".into()))?;
    let system: System = sqlx::query_as("SELECT * FROM systems WHERE id = $1")
        .bind(&system_id)
        .fetch_optional(&st.db)
        .await?
        .ok_or_else(|| ApiError::BadRequest("ไม่รู้จัก system ปลายทาง".into()))?;

    let dry = req.dry_run.unwrap_or(false);
    let limit = req.limit.unwrap_or(50).clamp(1, 500);
    let offset = req.offset.unwrap_or(0).max(0);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| ApiError::Other(anyhow::anyhow!(e)))?;
    let token = legacy_token(&client).await;

    // รายการที่จะนำเข้า: ระบุเองมา หรือดึงจากฮับเดิมทีละหน้า
    let mut rows: Vec<serde_json::Value> = Vec::new();
    if let Some(ids) = req.ids.clone() {
        for id in ids {
            let url = format!("{base}/FileService/getFileDetail?fileId={id}");
            let mut r = client.get(&url);
            if let Some(t) = &token { r = r.bearer_auth(t); }
            match r.send().await {
                Ok(resp) => match resp.json::<serde_json::Value>().await {
                    Ok(v) => { if let Some(d) = v.get("data") { rows.push(d.clone()); } }
                    Err(e) => return Err(ApiError::Other(anyhow::anyhow!("อ่านรายละเอียดไม่ได้: {e}"))),
                },
                Err(e) => return Err(ApiError::Other(anyhow::anyhow!("เรียกฮับเดิมไม่ได้: {e}"))),
            }
        }
    } else {
        let url = format!("{base}/FileService/getFiles?offset={offset}&limitOfset={limit}");
        let mut r = client.get(&url);
        if let Some(t) = &token { r = r.bearer_auth(t); }
        let v: serde_json::Value = r
            .send()
            .await
            .map_err(|e| ApiError::Other(anyhow::anyhow!("เรียกฮับเดิมไม่ได้: {e}")))?
            .json()
            .await
            .map_err(|e| ApiError::Other(anyhow::anyhow!("อ่านรายการไม่ได้: {e}")))?;
        if let Some(arr) = v.get("data").and_then(|d| d.as_array()) {
            rows = arr.clone();
        }
    }

    let mut out = ImportResult { dry_run: dry, ..Default::default() };
    for row in rows {
        out.scanned += 1;
        // โฟลเดอร์ของเดิมไม่ใช่ไฟล์ — ข้าม
        if s(&row, "type_").as_deref() == Some("DIRECTORY") {
            continue;
        }
        let Some(raw_id) = s(&row, "file_system_id").or_else(|| s(&row, "id")) else {
            out.failed += 1;
            out.errors.push("แถวไม่มีรหัสไฟล์".into());
            continue;
        };
        let Ok(id) = Uuid::parse_str(&raw_id) else {
            out.failed += 1;
            out.errors.push(format!("รหัสไม่ใช่ UUID: {raw_id}"));
            continue;
        };
        let exists: Option<(Uuid,)> = sqlx::query_as("SELECT id FROM files WHERE id = $1")
            .bind(id)
            .fetch_optional(&st.db)
            .await?;
        if exists.is_some() {
            out.skipped_existing += 1;
            continue;
        }
        let name = s(&row, "full_name_type")
            .or_else(|| s(&row, "fileName"))
            .or_else(|| s(&row, "file_name"))
            .unwrap_or_else(|| raw_id.clone());
        if dry {
            out.imported += 1;
            continue;
        }

        // ดึงไบต์จากฮับเดิม
        let url = format!("{base}/FileService/downloadFile?fileId={raw_id}&logType=download");
        let mut rq = client.get(&url);
        if let Some(t) = &token { rq = rq.bearer_auth(t); }
        let resp = match rq.send().await {
            Ok(r) => r,
            Err(e) => { out.failed += 1; out.errors.push(format!("{name}: โหลดไม่ได้ {e}")); continue; }
        };
        if !resp.status().is_success() {
            out.failed += 1;
            out.errors.push(format!("{name}: ฮับเดิมตอบ {}", resp.status()));
            continue;
        }
        let ct = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|v| v.split(';').next().unwrap_or(v).trim().to_string());
        let body = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => { out.failed += 1; out.errors.push(format!("{name}: อ่านไบต์ไม่ได้ {e}")); continue; }
        };

        let safe = sanitize_filename(&name);
        let object_key = format!("{}/{}-{}", system.bucket, id, safe);
        let etag = match st.storage.put(&object_key, body.clone(), ct.as_deref()).await {
            Ok(e) => e,
            Err(e) => { out.failed += 1; out.errors.push(format!("{name}: เขียนไม่ได้ {e}")); continue; }
        };
        let encrypted = st.storage.encryption_enabled();
        let file_type = crate::handlers::detect_file_type(&name);
        let size = body.len() as i64;

        // แท็กบอกที่มา + แท็กเดิมถ้ามี เพื่อให้ค้นย้อนกลับได้ว่าใบไหนมาจากฮับเดิม
        let mut tags: Vec<String> = vec!["นำเข้า:ฮับเดิม".into()];
        if let Some(t) = s(&row, "tag_name") {
            if let Ok(serde_json::Value::Array(a)) = serde_json::from_str::<serde_json::Value>(&t) {
                for x in a { if let Some(v) = x.as_str() { tags.push(v.to_string()); } }
            } else {
                tags.push(t);
            }
        }
        // ของเดิมเก็บ "เจ้าของ" เป็นรหัสผู้ใช้ดิบ ๆ ถ้ายกมาตรง ๆ หน้าจอจะโชว์ UUID
        // ยาวเหยียดซึ่งอ่านไม่รู้เรื่อง — เก็บรหัสเดิมไว้เป็นแท็กเพื่อสืบกลับได้
        // แล้วแสดงชื่อที่คนอ่านออกแทน
        let owner = "นำเข้าจากฮับเดิม".to_string();
        if let Some(uid) = s(&row, "owner_user_id") {
            tags.push(format!("ผู้ใช้เดิม:{uid}"));
        }

        let res = sqlx::query(
            r#"INSERT INTO files
                (id, name, file_type, size_bytes, system_id, org_id, folder_id, bucket, object_key,
                 project, status, owner, tags, version, metadata, etag, encrypted, created_at, modified_at, created_by)
               VALUES ($1,$2,$3,$4,$5,NULL,NULL,$6,$7,NULL,'Archived',$8,$9,1,'{}',$10,$11,now(),now(),$12)"#,
        )
        .bind(id)
        .bind(&name)
        .bind(&file_type)
        .bind(size)
        .bind(&system_id)
        .bind(&system.bucket)
        .bind(&object_key)
        .bind(&owner)
        .bind(serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into()))
        .bind(&etag)
        .bind(encrypted)
        .bind(&user.0.id)
        .execute(&st.db)
        .await;
        match res {
            Ok(_) => { out.imported += 1; out.bytes += size; }
            Err(e) => {
                out.failed += 1;
                out.errors.push(format!("{name}: บันทึกไม่ได้ {e}"));
                let _ = st.storage.delete(&object_key).await;
            }
        }
        if out.errors.len() > 20 { out.errors.truncate(20); }
    }
    tracing::info!(
        scanned = out.scanned, imported = out.imported, skipped = out.skipped_existing,
        failed = out.failed, "นำเข้าไฟล์จากฮับเดิม"
    );
    Ok(Json(out))
}
