//! ชั้นรองรับ API เดิม `/FileService/*` (NEB — 22 ก.ย. 2569)
//!
//! ## ทำไมต้องมีไฟล์นี้
//! ระบบงานของ NEB ราว 20 ตัว (DTS, BPN, BGA, BPP, พอร์ทัล, Report Studio ฯลฯ)
//! เรียกไฟล์ฮับตัวเดิมด้วย URL ชุดนี้อยู่แล้ว — `/FileService/upload`,
//! `/FileService/downloadFile?fileId=...` ฯลฯ รวมจุดเรียกกว่า 250 แห่ง
//! ถ้าจะย้ายมาใช้ FileHub ตัวใหม่แล้วต้องไล่แก้โค้ดทุกระบบ = แก้ 20 repo
//! ทดสอบใหม่ 20 รอบ และต้องปล่อยขึ้นพร้อมกันทั้งหมด
//!
//! ไฟล์นี้จึงทำให้ FileHub ตัวใหม่ "พูดภาษาเดิม" ได้ด้วย ⇒ ระบบเดิม
//! **ไม่ต้องแก้โค้ดแม้แต่บรรทัดเดียว** เปลี่ยนแค่ค่า URL ปลายทางใน
//! configmap/secret (FILE_SERVICE_URL / app.filehub.url / PGD_APP_URL_CENTER_TECH)
//! แล้วเรียกตัวใหม่ได้ทันที และย้ายทีละระบบได้ ไม่ต้องยกชุดเดียวพร้อมกัน
//!
//! ## รูปแบบคำตอบ
//! คัดลอกจากของจริงบน UAT (`neb-center-filehub-svc:7880`) ทีละคีย์ ทั้ง
//! `{success,status,message,data:{...}}` ของ upload/getFileDetail และ
//! `{success,message,total,data:[...]}` ของ getFiles เพราะฝั่งผู้เรียกอ่านคีย์
//! ตรง ๆ เช่น `data.id`, `data.file_name`, `data.mime_type`, `data.file_path`
//! (ดู `owdropzone` ของ OneWeb และ `FileHubUploadResponse` ของ DTS)
//!
//! ## ไฟล์เก่ายังโหลดได้
//! `fileId` ของไฟล์ที่อัปโหลดไว้กับตัวเดิมไม่มีใน FileHub ใหม่ ⇒ ถ้าหาในฐานไม่เจอ
//! และตั้ง `LEGACY_FILEHUB_URL` ไว้ จะส่งต่อคำขอไปตัวเดิมให้อัตโนมัติ
//! ระบบที่ย้ายมาแล้วจึงยังเปิดไฟล์เก่าได้ตามปกติ ไม่ต้องรอย้ายข้อมูล
//!
//! ## ตัวตนผู้เรียก
//! เรียงลำดับ: ตัวตนจากขอบนอก/API key ของ FileHub ใหม่ (ผ่าน `MaybeAuthUser`)
//! → ถ้าไม่มี ใช้บัญชีบริการสำหรับระบบเดิม เพราะของเดิมส่ง Bearer เป็น token
//! ของ center ซึ่งตัวใหม่ตรวจไม่ได้ (และนั่นคือสิ่งที่เราไม่อยากให้ต้องแก้)
//!
//! 🔴 ข้อบังคับเรื่องความปลอดภัย: เส้นทาง `/FileService/*` ต้องไม่เปิดออก
//! อินเทอร์เน็ตตรง ๆ ให้เรียกได้เฉพาะภายในคลัสเตอร์ (ClusterIP) หรือผ่าน
//! oauth2-proxy เท่านั้น — ไม่งั้นจะซ้ำรอยไฟล์ฮับตัวเดิมที่ใครก็โหลดไฟล์ได้
//! ปิดทั้งชั้นนี้ได้ด้วยการไม่ตั้ง `LEGACY_FILESERVICE=1`

use std::sync::Arc;

use axum::{
    extract::{DefaultBodyLimit, Multipart, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::auth::{MaybeAuthUser, User};
use crate::error::{ApiError, ApiResult};
use crate::handlers::{persist_upload_for, sanitize_filename, UploadFields, FILE_COLS};
use crate::models::File;
use crate::AppState;

/// เปิดใช้ชั้นรองรับของเดิมหรือไม่ (ค่าเริ่มต้น: ปิด)
pub fn enabled() -> bool {
    matches!(
        std::env::var("LEGACY_FILESERVICE").unwrap_or_default().trim(),
        "1" | "true" | "on" | "yes"
    )
}

fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

/// ปลายทางไฟล์ฮับตัวเดิม ใช้ส่งต่อคำขอของไฟล์เก่าที่ยังไม่ได้ย้ายมา
fn legacy_upstream() -> Option<String> {
    env_opt("LEGACY_FILEHUB_URL").map(|v| v.trim_end_matches('/').to_string())
}

/// เพดานขนาดไฟล์ของเส้นเก่า (ค่าเริ่มต้น 256 MB)
fn legacy_max_upload_bytes() -> usize {
    env_opt("LEGACY_MAX_UPLOAD_MB")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(256)
        * 1024
        * 1024
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/FileService/upload", post(upload))
        .route("/FileService/uploadFile", post(upload))
        .route("/FileService/downloadFile", get(download))
        .route("/FileService/previewFile", get(preview))
        .route("/FileService/getFileDetail", get(file_detail))
        .route("/FileService/getFiles", get(get_files))
        .route("/FileService/rename", post(rename))
        .route("/FileService/moveFileToTrash", post(move_to_trash))
        // ขอบนอกของเดิมตั้ง client-max-body-size ไว้ 2048m ⇒ ถ้าตั้งต่ำกว่านั้นมาก
        // ไฟล์ใหญ่ที่เคยอัปได้จะเริ่มล้มหลังสลับปลายทาง (regression ที่คนจะโทษ FileHub ใหม่)
        // ตัวเลขนี้กินแรมจริงต่อคำขอ เพราะอ่านทั้งก้อนก่อนเขียน จึงตั้งค่าได้ด้วย
        // LEGACY_MAX_UPLOAD_MB แล้วปรับ memory limit ของพ็อดให้สัมพันธ์กัน
        .layer(DefaultBodyLimit::max(legacy_max_upload_bytes()))
}

/// ระบบปลายทางของไฟล์ที่เข้ามาทางเส้นเดิม — "ถัง" ที่ไฟล์จะไปวาง
///
/// ของเดิมเทไฟล์ทุกระบบรวมกองเดียว ทำให้แยกโควตา/อายุเก็บ/สิทธิ์รายระบบไม่ได้
/// ตัวนี้เดาระบบต้นทางให้เองโดยที่ผู้เรียก **ไม่ต้องแก้โค้ด**:
///   1. เฮดเดอร์ `x-filehub-system` (ถ้าใครอยากระบุตรง ๆ)
///   2. `Referer` ของจอที่กดอัปโหลด — จอของ NEB อยู่ใต้ path ของโมดูลตัวเอง
///      เช่น /neb-upm/... ⇒ ถังของ UPM   (ครอบการอัปโหลดจากเบราว์เซอร์เกือบทั้งหมด)
///   3. ค่าตั้งต้น `LEGACY_DEFAULT_SYSTEM`
///
/// แม็ปตั้งใน `LEGACY_SYSTEM_MAP` เช่น "neb-upm=sys_upm,digital-signature=sys_dts"
/// (คั่นด้วย , ) — เพิ่มโมดูลใหม่ทีหลังได้โดยไม่ต้อง build ใหม่
fn system_from_request(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-filehub-system").and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    let referer = headers
        .get(header::REFERER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !referer.is_empty() {
        for pair in env_opt("LEGACY_SYSTEM_MAP").unwrap_or_default().split(',') {
            let (prefix, system) = match pair.split_once('=') {
                Some((a, b)) => (a.trim().to_lowercase(), b.trim().to_string()),
                None => continue,
            };
            if prefix.is_empty() || system.is_empty() {
                continue;
            }
            // เทียบเฉพาะส่วน path ไม่ให้ชื่อโฮสต์มาชนโดยบังเอิญ
            if referer.contains(&format!("/{prefix}/")) || referer.ends_with(&format!("/{prefix}")) {
                return Some(system);
            }
        }
    }
    None
}

// ────────────────────────── ตัวตนของผู้เรียก ──────────────────────────

/// บัญชีบริการสำหรับระบบเดิม — สร้างครั้งเดียวแล้วใช้ซ้ำ
///
/// ระบบเดิมส่ง `Authorization: Bearer <token ของ center>` มาด้วย แต่ตัวใหม่
/// ตรวจ token นั้นไม่ได้ (คนละระบบตัวตน) จะให้แก้โค้ดฝั่งผู้เรียกก็ผิดโจทย์
/// จึงยอมรับคำขอที่เข้ามาถึงเส้นทางนี้ว่าเป็น "ระบบเดิม" และบันทึกเป็นบัญชีนี้
/// ให้ตรวจสอบย้อนหลังได้ว่าไฟล์ไหนมาทางเส้นเก่า
async fn legacy_service_user(s: &Arc<AppState>) -> ApiResult<User> {
    const ID: &str = "usr_legacy_fileservice";
    if let Some(u) = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(ID)
        .fetch_optional(&s.db)
        .await?
    {
        return Ok(u);
    }
    let role = env_opt("LEGACY_SERVICE_ROLE").unwrap_or_else(|| "editor".into());
    let system_id = env_opt("LEGACY_DEFAULT_SYSTEM").or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"));
    let org_id = env_opt("LEGACY_DEFAULT_ORG");
    sqlx::query(
        r#"INSERT INTO users (id, email, display_name, avatar_tone, password_hash, role, status,
                              default_system_id, default_org_id, source)
           VALUES ($1, 'fileservice@legacy.local', 'ระบบเดิม (FileService)', 'slate', '', $2,
                   'active', $3, $4, 'legacy')
           ON CONFLICT (id) DO NOTHING"#,
    )
    .bind(ID)
    .bind(&role)
    .bind(&system_id)
    .bind(&org_id)
    .execute(&s.db)
    .await?;
    Ok(sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(ID)
        .fetch_one(&s.db)
        .await?)
}

/// คืนผู้ใช้ที่จะใช้บันทึก — คนจริงถ้ารู้จัก ไม่งั้นเป็นบัญชีบริการของระบบเดิม
async fn caller(s: &Arc<AppState>, who: Option<User>) -> ApiResult<User> {
    match who {
        Some(u) => Ok(u),
        None => {
            // ปิดทางสำรองนี้ได้ด้วย LEGACY_TRUST_CALLER=0 เมื่อย้ายผู้เรียกครบแล้ว
            if env_opt("LEGACY_TRUST_CALLER").as_deref() == Some("0") {
                return Err(ApiError::Unauthorized);
            }
            legacy_service_user(s).await
        }
    }
}

// ────────────────────────── รูปคำตอบแบบเดิม ──────────────────────────

fn th_date(d: &DateTime<Utc>) -> String {
    d.with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
        .format("%d %B %Y")
        .to_string()
}

fn real_ts(d: &DateTime<Utc>) -> String {
    d.with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap())
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string()
}

/// ชื่อไฟล์ที่ตัดนามสกุลออก — ของเดิมคืน `file_name` แบบไม่มีนามสกุล
/// และคืนชื่อเต็มไว้ที่ `fileName` / `full_name_type` ต้องเหมือนกันเป๊ะ
/// เพราะบางระบบเอา `file_name` ไปต่อนามสกุลเองแล้ว
fn stem(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

fn download_link(id: &Uuid) -> String {
    match env_opt("PUBLIC_BASE_URL") {
        Some(base) => format!("{}/FileService/downloadFile?fileId={}", base.trim_end_matches('/'), id),
        None => format!("/FileService/downloadFile?fileId={id}"),
    }
}

/// แปลงไฟล์ของเราให้เป็นก้อน JSON หน้าตาเดียวกับของเดิมทุกคีย์
fn legacy_file(f: &File) -> Value {
    let mime = mime_guess::from_path(&f.name).first().map(|m| m.to_string());
    json!({
        "id": f.id,
        "fileType": f.file_type,
        "fileName": f.name,
        "full_name_type": f.name,
        "file_name": stem(&f.name),
        "file_type": f.file_type,
        "mime_type": mime,
        "file_path": "",
        "file_system_id": f.id,
        "file_app_id": f.id,
        "size_": f.size_bytes.to_string(),
        "type_": "FILE",
        "active_": "Y",
        "file_version": f.version.to_string(),
        "prefix_version": "1",
        "control_version": null,
        "encrypt_type": if f.encrypted { "AES256" } else { "DEFAULT" },
        "parent_folder_id": f.folder_id.clone().unwrap_or_default(),
        "group_id": "",
        "link_download": "",
        "linkFile": download_link(&f.id),
        "user_id": f.created_by.clone().unwrap_or_default(),
        "owner_user_id": f.created_by.clone().unwrap_or_default(),
        "create_date": th_date(&f.created_at),
        "update_date": th_date(&f.modified_at),
        "create_date_real": real_ts(&f.created_at),
        "update_date_real": real_ts(&f.modified_at),
        "tag_name": f.tags,
        "module_name": f.project,
        "department_name": f.org_id,
        "permission_anyone": "anyoneWithTheLink",
        "permission_file": "viewer",
        "permissionsGranted": true,
        "hash_file": f.etag,
        "risk_virus": null,
        "sign_off": null,
        "sync_status": null,
        "checkOutObj": null,
        "share_with_user_id": null,
        "task_id": null,
        "ignore": false,
    })
}

// ────────────────────────── อัปโหลด ──────────────────────────

#[derive(Default)]
struct LegacyUpload {
    name: Option<String>,
    body: Option<bytes::Bytes>,
    content_type: Option<String>,
    parent_folder_id: Option<String>,
    file_type: Option<String>,
    group_id: Option<String>,
}

/// อ่าน multipart แบบของเดิม
///
/// ชื่อฟิลด์ไฟล์ไม่แน่นอน: ฝั่ง Angular ส่ง `file`, ฝั่ง Java บางตัวส่งชื่อว่างเปล่า
/// และ 598-app ส่งชื่อฟิลด์ตามที่ผู้เรียกกำหนดเอง ⇒ ถือว่า "ฟิลด์ไหนที่มีชื่อไฟล์
/// ติดมาด้วย คือไฟล์" ไม่ยึดชื่อฟิลด์
async fn read_legacy_upload(mp: &mut Multipart) -> ApiResult<LegacyUpload> {
    let mut out = LegacyUpload::default();
    while let Some(mut field) = mp
        .next_field()
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?
    {
        let fname = field.file_name().map(str::to_string);
        let key = field.name().unwrap_or("").to_string();
        if fname.is_some() && out.body.is_none() {
            out.name = fname;
            out.content_type = field.content_type().map(str::to_string);
            let mut buf = bytes::BytesMut::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|e| ApiError::BadRequest(e.to_string()))?
            {
                buf.extend_from_slice(&chunk);
            }
            out.body = Some(buf.freeze());
            continue;
        }
        let val = field.text().await.unwrap_or_default();
        if val.trim().is_empty() {
            continue;
        }
        match key.as_str() {
            "fileName" => out.name.get_or_insert(val),
            "parentFolderId" | "parent_folder_id" => out.parent_folder_id.insert(val),
            "fileType" => out.file_type.insert(val),
            "groupId" => out.group_id.insert(val),
            _ => continue,
        };
    }
    Ok(out)
}

async fn upload(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    headers: HeaderMap,
    mut mp: Multipart,
) -> ApiResult<Json<Value>> {
    let user = caller(&s, who).await?;
    crate::auth::require_role(&user, &["admin", "editor"])?;
    crate::auth::upload_rate_limit(&user.id).await?;

    let up = read_legacy_upload(&mut mp).await?;
    let name = up.name.clone().ok_or_else(|| ApiError::BadRequest("missing file".into()))?;
    let body = up.body.clone().ok_or_else(|| ApiError::BadRequest("missing file body".into()))?;

    // โฟลเดอร์: ของเดิมส่งรหัสโฟลเดอร์ของตัวเองมา (เช่น parentFolderId ของ owdropzone)
    // ถ้าไม่รับไว้ ไฟล์จะไปกองที่ราก แล้วผู้เรียกที่ list ด้วย parentFolderId จะหาไม่เจอ
    // จึง "จองรหัสเดิมไว้" — สร้างโฟลเดอร์ที่ใช้ id เดียวกับของเดิม (id เป็น text)
    // ⇒ getFiles?parentFolderId=<รหัสเดิม> ยังคืนไฟล์ได้เหมือนเดิม
    let folder_id = match up.parent_folder_id.as_deref() {
        Some(v) => Some(ensure_legacy_folder(&s, v, &user, system_from_request(&headers).as_deref()).await?),
        None => None,
    };

    // แท็กเพิ่มจากข้อมูลที่ของเดิมส่งมาอยู่แล้ว + ป้ายบอกว่ามาทางเส้นเก่า
    // (ถ้าไม่ส่ง tags มาเลย persist_upload_for จะติดแท็กอัตโนมัติให้ต่ออีกชุด)
    let mut extra: Vec<String> = vec!["ผ่าน:FileService เดิม".into()];
    if let Some(v) = up.file_type.as_deref() {
        extra.push(format!("ประเภท:{v}"));
    }
    if let Some(v) = up.group_id.as_deref() {
        extra.push(format!("กลุ่ม:{v}"));
    }

    let fields = UploadFields {
        name: Some(name),
        body: Some(body),
        content_type: up.content_type,
        folder_id,
        system_id: system_from_request(&headers),
        org_id: None,
        project: None,
        status: None,
        owner: None,
        tags: None,
    };
    let file = persist_upload_for(&s, Some(user.id.as_str()), fields, Some(&user)).await?;
    // ต่อแท็กของเส้นเก่าเข้าไปกับแท็กอัตโนมัติที่เพิ่งติดให้ แล้วอ่านกลับมาใหม่
    // เพื่อให้ `tag_name` ในคำตอบตรงกับที่บันทึกจริง (ผู้เรียกบางตัวเก็บค่านี้ไว้)
    append_tags(&s, &file, &extra).await?;
    let file = find_file(&s, &file.id.to_string()).await?.unwrap_or(file);

    Ok(Json(json!({
        "success": true,
        "status": 200,
        "message": "Upload file success.",
        "data": legacy_file(&file),
    })))
}

/// จองรหัสโฟลเดอร์ของระบบเดิมไว้ในฐานใหม่ (ใช้ id เดิมตรง ๆ) ถ้ายังไม่มี
async fn ensure_legacy_folder(
    s: &Arc<AppState>,
    legacy_id: &str,
    user: &User,
    want_system: Option<&str>,
) -> ApiResult<String> {
    if let Some(id) = sqlx::query_scalar::<_, String>("SELECT id FROM folders WHERE id = $1")
        .bind(legacy_id)
        .fetch_optional(&s.db)
        .await?
    {
        return Ok(id);
    }
    let system_id: Option<String> =
        sqlx::query_scalar("SELECT default_system_id FROM users WHERE id = $1")
            .bind(&user.id)
            .fetch_optional(&s.db)
            .await?
            .flatten();
    let system_id = want_system
        .map(|v| v.to_string())
        .or(system_id)
        .or_else(|| env_opt("LEGACY_DEFAULT_SYSTEM"))
        .or_else(|| env_opt("EDGE_DEFAULT_SYSTEM"))
        .ok_or_else(|| ApiError::BadRequest("missing system for folder".into()))?;
    sqlx::query(
        r#"INSERT INTO folders (id, system_id, name, owner, created_by)
           VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING"#,
    )
    .bind(legacy_id)
    .bind(&system_id)
    .bind(format!("โฟลเดอร์เดิม {legacy_id}"))
    .bind(&user.display_name)
    .bind(&user.id)
    .execute(&s.db)
    .await?;
    Ok(legacy_id.to_string())
}

async fn append_tags(s: &Arc<AppState>, f: &File, extra: &[String]) -> ApiResult<()> {
    let mut tags: Vec<String> = serde_json::from_str(&f.tags).unwrap_or_default();
    for t in extra {
        if !tags.iter().any(|x| x == t) {
            tags.push(t.clone());
        }
    }
    let json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
    sqlx::query("UPDATE files SET tags = $1 WHERE id = $2")
        .bind(&json)
        .bind(f.id)
        .execute(&s.db)
        .await?;
    Ok(())
}

// ────────────────────────── อ่านไฟล์ ──────────────────────────

#[derive(Deserialize)]
struct FileIdQuery {
    #[serde(rename = "fileId")]
    file_id: Option<String>,
    #[serde(rename = "logType")]
    _log_type: Option<String>,
}

/// หาไฟล์จาก `fileId` ของเดิม — คืน None เมื่อไม่ใช่ของฐานนี้ (ไฟล์เก่า)
async fn find_file(s: &Arc<AppState>, file_id: &str) -> ApiResult<Option<File>> {
    let Ok(id) = Uuid::parse_str(file_id) else { return Ok(None) };
    Ok(sqlx::query_as::<_, File>(&format!(
        "SELECT {FILE_COLS} FROM files WHERE id = $1 AND deleted_at IS NULL"
    ))
    .bind(id)
    .fetch_optional(&s.db)
    .await?)
}

/// ส่งต่อคำขอไปไฟล์ฮับตัวเดิม สำหรับไฟล์ที่อัปโหลดไว้ก่อนย้ายระบบ
async fn proxy_legacy(path_and_query: &str, headers: &HeaderMap) -> ApiResult<Response> {
    let Some(base) = legacy_upstream() else { return Err(ApiError::NotFound) };
    let url = format!("{base}{path_and_query}");
    let mut req = reqwest::Client::new().get(&url);
    if let Some(auth) = headers.get(header::AUTHORIZATION) {
        req = req.header(header::AUTHORIZATION, auth.clone());
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ApiError::Other(anyhow::anyhow!("legacy upstream: {e}")))?;
    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let mut out = HeaderMap::new();
    for k in [header::CONTENT_TYPE, header::CONTENT_DISPOSITION, header::CACHE_CONTROL] {
        if let Some(v) = resp.headers().get(k.as_str()) {
            if let Ok(hv) = axum::http::HeaderValue::from_bytes(v.as_bytes()) {
                out.insert(k, hv);
            }
        }
    }
    let body: bytes::Bytes = resp
        .bytes()
        .await
        .map_err(|e| ApiError::Other(anyhow::anyhow!("legacy upstream body: {e}")))?;
    Ok((status, out, body).into_response())
}

async fn serve_bytes(
    s: &Arc<AppState>,
    user: &User,
    f: &File,
    inline: bool,
) -> ApiResult<Response> {
    if crate::auth::ensure_system_access(&s.db, user, &f.system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let (body, ct) = s
        .storage
        .get(&f.object_key, f.encrypted)
        .await?
        .ok_or(ApiError::NotFound)?;
    let content_type = ct
        .or_else(|| mime_guess::from_path(&f.name).first().map(|m| m.to_string()))
        .unwrap_or_else(|| "application/octet-stream".into());
    let kind = if inline { "inline" } else { "attachment" };
    let ascii = sanitize_filename(&f.name);
    let utf8 = urlencoding::encode(&f.name);
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    h.insert(
        header::CONTENT_DISPOSITION,
        format!("{kind}; filename=\"{ascii}\"; filename*=UTF-8''{utf8}")
            .parse()
            .unwrap(),
    );
    Ok((StatusCode::OK, h, body).into_response())
}

async fn download(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Query(q): Query<FileIdQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let file_id = q.file_id.unwrap_or_default();
    if file_id.is_empty() {
        return Err(ApiError::BadRequest("fileId is required".into()));
    }
    match find_file(&s, &file_id).await? {
        Some(f) => {
            let user = caller(&s, who).await?;
            serve_bytes(&s, &user, &f, false).await
        }
        None => {
            proxy_legacy(
                &format!("/FileService/downloadFile?fileId={file_id}&logType=download"),
                &headers,
            )
            .await
        }
    }
}

async fn preview(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Query(q): Query<FileIdQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let file_id = q.file_id.unwrap_or_default();
    if file_id.is_empty() {
        return Err(ApiError::BadRequest("fileId is required".into()));
    }
    match find_file(&s, &file_id).await? {
        Some(f) => {
            let user = caller(&s, who).await?;
            serve_bytes(&s, &user, &f, true).await
        }
        None => {
            proxy_legacy(
                &format!("/FileService/previewFile?fileId={file_id}&logType=preview"),
                &headers,
            )
            .await
        }
    }
}

async fn file_detail(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Query(q): Query<FileIdQuery>,
    headers: HeaderMap,
) -> ApiResult<Response> {
    let file_id = q.file_id.unwrap_or_default();
    if file_id.is_empty() {
        return Err(ApiError::BadRequest("fileId is required".into()));
    }
    match find_file(&s, &file_id).await? {
        Some(f) => {
            let user = caller(&s, who).await?;
            if crate::auth::ensure_system_access(&s.db, &user, &f.system_id).await.is_err() {
                return Err(ApiError::NotFound);
            }
            Ok(Json(json!({
                "success": true,
                "message": "getFileDetail Successfully.",
                "data": legacy_file(&f),
            }))
            .into_response())
        }
        None => {
            proxy_legacy(&format!("/FileService/getFileDetail?fileId={file_id}"), &headers).await
        }
    }
}

// ────────────────────────── รายการไฟล์ ──────────────────────────

#[derive(Deserialize)]
struct GetFilesQuery {
    offset: Option<i64>,
    #[serde(rename = "limitOfset")]
    limit_ofset: Option<i64>,
    keyword: Option<String>,
    tag: Option<String>,
    #[serde(rename = "parentFolderId")]
    parent_folder_id: Option<String>,
}

async fn get_files(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Query(q): Query<GetFilesQuery>,
) -> ApiResult<Json<Value>> {
    let user = caller(&s, who).await?;
    let limit = q.limit_ofset.unwrap_or(50).clamp(1, 500);
    let offset = q.offset.unwrap_or(0).max(0);

    let mut sql = String::from(" WHERE deleted_at IS NULL");
    let mut binds: Vec<String> = Vec::new();
    let mut n = 0usize;
    if let Some(k) = q.keyword.filter(|v| !v.trim().is_empty()) {
        n += 1;
        sql.push_str(&format!(" AND name ILIKE ${n}"));
        binds.push(format!("%{k}%"));
    }
    if let Some(t) = q.tag.filter(|v| !v.trim().is_empty()) {
        n += 1;
        sql.push_str(&format!(" AND tags ILIKE ${n}"));
        binds.push(format!("%{t}%"));
    }
    if let Some(p) = q.parent_folder_id.filter(|v| !v.trim().is_empty()) {
        n += 1;
        sql.push_str(&format!(" AND folder_id = ${n}"));
        binds.push(p);
    }
    // เห็นได้เฉพาะระบบที่ผู้เรียกมีสิทธิ์ — กติกาเดียวกับ /api/files
    let scope = crate::auth::effective_system_ids(&s.db, &user).await?;
    let mut scope_bind: Option<Vec<String>> = None;
    if let Some(ids) = scope {
        n += 1;
        sql.push_str(&format!(" AND system_id = ANY(${n})"));
        scope_bind = Some(ids);
    }

    let count_sql = format!("SELECT COUNT(*) FROM files{sql}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    if let Some(ref ids) = scope_bind {
        cq = cq.bind(ids);
    }
    let total: i64 = cq.fetch_one(&s.db).await?;

    let page_sql = format!(
        "SELECT {FILE_COLS} FROM files{sql} ORDER BY modified_at DESC, id DESC LIMIT ${} OFFSET ${}",
        n + 1,
        n + 2
    );
    let mut pq = sqlx::query_as::<_, File>(&page_sql);
    for b in &binds {
        pq = pq.bind(b);
    }
    if let Some(ref ids) = scope_bind {
        pq = pq.bind(ids);
    }
    let files = pq.bind(limit).bind(offset).fetch_all(&s.db).await?;

    Ok(Json(json!({
        "success": true,
        "message": "get file Successfully.",
        "total": total,
        "data": files.iter().map(legacy_file).collect::<Vec<_>>(),
    })))
}

// ────────────────────────── แก้ชื่อ / ทิ้งลงถังขยะ ──────────────────────────

#[derive(Deserialize)]
struct RenameBody {
    #[serde(rename = "fileId")]
    file_id: String,
    #[serde(rename = "renameTo")]
    rename_to: String,
}

async fn rename(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Json(b): Json<RenameBody>,
) -> ApiResult<Json<Value>> {
    let user = caller(&s, who).await?;
    crate::auth::require_role(&user, &["admin", "editor"])?;
    let f = find_file(&s, &b.file_id).await?.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user, &f.system_id).await?;
    if b.rename_to.trim().is_empty() {
        return Err(ApiError::BadRequest("renameTo is required".into()));
    }
    sqlx::query("UPDATE files SET name = $1, modified_at = now() WHERE id = $2")
        .bind(b.rename_to.trim())
        .bind(f.id)
        .execute(&s.db)
        .await?;
    Ok(Json(json!({
        "success": true,
        "status": 200,
        "message": "rename successfully",
    })))
}

#[derive(Deserialize)]
struct TrashBody {
    #[serde(rename = "fileId")]
    file_id: String,
}

async fn move_to_trash(
    State(s): State<Arc<AppState>>,
    MaybeAuthUser(who): MaybeAuthUser,
    Json(b): Json<TrashBody>,
) -> ApiResult<Json<Value>> {
    let user = caller(&s, who).await?;
    crate::auth::require_role(&user, &["admin", "editor"])?;
    let f = find_file(&s, &b.file_id).await?.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user, &f.system_id).await?;
    sqlx::query("UPDATE files SET deleted_at = now() WHERE id = $1")
        .bind(f.id)
        .execute(&s.db)
        .await?;
    sqlx::query(
        r#"INSERT INTO activity (id, actor, actor_tone, action, target, target_type, file_id,
                                 system_id, org_id, created_at, actor_id)
           VALUES ($1,$2,'slate','deleted',$3,$4,$5,$6,$7,now(),$8)"#,
    )
    .bind(Uuid::now_v7())
    .bind(&user.display_name)
    .bind(&f.name)
    .bind(&f.file_type)
    .bind(f.id)
    .bind(&f.system_id)
    .bind(&f.org_id)
    .bind(&user.id)
    .execute(&s.db)
    .await?;
    Ok(Json(json!({
        "status": 200,
        "success": true,
        "message": format!("move file to trash:{} successfully", b.file_id),
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_strips_only_the_last_extension() {
        // ของเดิมคืน file_name แบบไม่มีนามสกุล และคืนชื่อเต็มที่ fileName
        assert_eq!(stem("probe-compat.txt"), "probe-compat");
        assert_eq!(stem("รายงาน.งบ.2570.pdf"), "รายงาน.งบ.2570");
        assert_eq!(stem("no-extension"), "no-extension");
        // ไฟล์ซ่อนของยูนิกซ์ไม่ใช่ "นามสกุลล้วน" — ต้องไม่เหลือชื่อว่าง
        assert_eq!(stem(".env"), ".env");
    }

    #[test]
    fn download_link_uses_the_legacy_shape() {
        std::env::remove_var("PUBLIC_BASE_URL");
        let id = Uuid::nil();
        assert_eq!(
            download_link(&id),
            format!("/FileService/downloadFile?fileId={id}")
        );
    }

    #[test]
    fn enabled_only_on_explicit_opt_in() {
        std::env::remove_var("LEGACY_FILESERVICE");
        assert!(!enabled());
        std::env::set_var("LEGACY_FILESERVICE", "1");
        assert!(enabled());
        std::env::set_var("LEGACY_FILESERVICE", "0");
        assert!(!enabled());
        std::env::remove_var("LEGACY_FILESERVICE");
    }
}
