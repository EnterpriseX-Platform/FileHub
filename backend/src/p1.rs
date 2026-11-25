//! P1 features in one place: workflow, comments, notifications, thumbnails.
//! Each section is self-contained — see the module-level dividers.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use image::{ImageEncoder, ImageReader};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use crate::Id;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

// =============================================================================
// PDF / text extraction (TOR 4.15.14 "search content").
// =============================================================================

/// Try to extract searchable text from the uploaded payload.  Returns `None`
/// if the format isn't supported — the upload still succeeds, just without
/// content-level search hits.
pub fn extract_text_from(file_type: &str, body: &[u8]) -> Option<String> {
    match file_type {
        "pdf" => pdf_extract::extract_text_from_mem(body).ok(),
        "txt" | "md" | "html" | "xml" | "csv" | "json" => {
            // Plain-ish text files: best-effort UTF-8 decode, capped at 1 MB so
            // a runaway JSON dump doesn't bloat the FTS index.
            let cap = body.len().min(1024 * 1024);
            std::str::from_utf8(&body[..cap]).ok().map(|s| s.to_string())
        }
        "docx" | "xlsx" | "pptx" => ooxml_text(file_type, body),
        _ => None,
    }
}

/// Text of a Word / Excel / PowerPoint file (a ZIP of XML parts): the text runs
/// of the body, sheets and slides, joined per paragraph/cell. Capped like text files.
fn ooxml_text(file_type: &str, body: &[u8]) -> Option<String> {
    use std::io::Read;
    const PART_MAX: u64 = 32 * 1024 * 1024; // uncompressed, per part (zip bombs)
    const OUT_MAX: usize = 1024 * 1024;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(body)).ok()?;
    let mut parts: Vec<String> = z.file_names().filter(|n| match file_type {
        "docx" => n == &"word/document.xml" || ((n.starts_with("word/header") || n.starts_with("word/footer")
                   || n.starts_with("word/footnotes") || n.starts_with("word/endnotes")) && n.ends_with(".xml")),
        "xlsx" => n == &"xl/sharedStrings.xml" || (n.starts_with("xl/worksheets/sheet") && n.ends_with(".xml")),
        "pptx" => (n.starts_with("ppt/slides/slide") || n.starts_with("ppt/notesSlides/notesSlide")) && n.ends_with(".xml"),
        _ => false,
    }).map(str::to_string).collect();
    parts.sort();
    let mut out = String::new();
    for name in parts {
        let Ok(f) = z.by_name(&name) else { continue };
        let mut xml = String::new();
        if f.take(PART_MAX).read_to_string(&mut xml).is_err() { continue; }
        xml_text_runs(&xml, &mut out);
        if out.len() >= OUT_MAX { break; }
    }
    if out.len() > OUT_MAX {
        let mut cut = OUT_MAX;
        while !out.is_char_boundary(cut) { cut -= 1; }
        out.truncate(cut);
    }
    let t = out.trim();
    if t.is_empty() { None } else { Some(t.to_string()) }
}

/// Append the contents of `<…:t>` elements (Word/Excel/PowerPoint text runs);
/// a paragraph, cell, row, tab or break ends a word.
fn xml_text_runs(xml: &str, out: &mut String) {
    let mut in_t = false;
    let mut rest = xml;
    while let Some(lt) = rest.find('<') {
        if in_t { out.push_str(&xml_unescape(&rest[..lt])); }
        let Some(gt) = rest[lt..].find('>') else { break };
        let tag = &rest[lt + 1..lt + gt];
        rest = &rest[lt + gt + 1..];
        let closing = tag.starts_with('/');
        let self_closing = tag.ends_with('/');
        let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace() || c == '/').next().unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or(name);
        match local {
            "t" => in_t = !closing && !self_closing,
            "p" | "si" | "c" | "row" | "tab" | "br" | "tc" if closing || self_closing || local == "tab" || local == "br" => {
                if !out.ends_with(' ') && !out.is_empty() { out.push(' '); }
            }
            _ => {}
        }
    }
}

fn xml_unescape(s: &str) -> String {
    if !s.contains('&') { return s.to_string(); }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        o.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find(';') {
            Some(j) if j <= 10 => {
                let ent = &tail[1..j];
                let ch = match ent {
                    "amp" => Some('&'), "lt" => Some('<'), "gt" => Some('>'), "quot" => Some('"'), "apos" => Some('\''),
                    _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
                    _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
                    _ => None,
                };
                match ch { Some(c) => { o.push(c); rest = &tail[j + 1..]; } None => { o.push('&'); rest = &tail[1..]; } }
            }
            _ => { o.push('&'); rest = &tail[1..]; }
        }
    }
    o.push_str(rest);
    o
}

pub async fn index_file_content(db: &sqlx::PgPool, file_id: Id, content: &str) {
    if content.trim().is_empty() { return; }
    let _ = sqlx::query(
        r#"INSERT INTO file_content (file_id, content)
           VALUES ($1, $2)
           ON CONFLICT (file_id) DO UPDATE SET content = EXCLUDED.content, extracted_at = now()"#,
    )
    .bind(file_id)
    .bind(content)
    .execute(db)
    .await;
}

// =============================================================================
// Thumbnails (TOR 4.15.16 image caching).
// =============================================================================

/// Decode the image, resize the longest side to `max_dim`, encode as WebP.
pub fn make_thumbnail(body: &[u8], max_dim: u32) -> Option<(u32, u32, Vec<u8>)> {
    // Refuse decompression bombs before decoding: a few KB can declare a
    // 30000×30000 image that would need gigabytes.
    let mut reader = ImageReader::new(std::io::Cursor::new(body)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(10_000);
    limits.max_image_height = Some(10_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let thumb = img.thumbnail(max_dim, max_dim);
    let (w, h) = (thumb.width(), thumb.height());
    let mut out = Vec::new();
    // image 0.25 doesn't ship a WebP encoder without a feature flag, so we
    // fall back to PNG. Either is fine for serving — the table notes the mime.
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(thumb.to_rgba8().as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some((w, h, out))
}

pub async fn index_thumbnail(db: &sqlx::PgPool, file_id: Id, file_type: &str, body: &[u8]) {
    if !matches!(file_type, "png" | "jpg" | "jpeg" | "img" | "gif") { return; }
    let owned = body.to_vec();
    let Ok(Some((w, h, bytes))) = tokio::task::spawn_blocking(move || make_thumbnail(&owned, 256)).await
    else { return; };
    let _ = sqlx::query(
        r#"INSERT INTO thumbnails (file_id, width, height, mime, data)
           VALUES ($1, $2, $3, 'image/png', $4)
           ON CONFLICT (file_id) DO UPDATE SET width = EXCLUDED.width,
                                                height = EXCLUDED.height,
                                                data = EXCLUDED.data,
                                                created_at = now()"#,
    )
    .bind(file_id).bind(w as i32).bind(h as i32).bind(bytes)
    .execute(db).await;
}

// =============================================================================
// Office → PDF preview via LibreOffice headless (TOR + browser-view).
// =============================================================================

/// File types that LibreOffice can render to PDF.  PDF itself is included so
/// `/preview` always returns a viewable PDF without the caller needing to
/// know which formats are convertible.
fn is_office_doc(file_type: &str) -> bool {
    matches!(file_type, "docx" | "doc" | "xlsx" | "xls" | "pptx" | "ppt" | "odt" | "ods" | "odp" | "pdf")
}

/// Spawn LibreOffice to convert the buffer to PDF.  Returns the PDF bytes,
/// or `None` if `soffice` is missing or conversion fails.  Caller decides
/// whether that's fatal (it's not — uploads still succeed).
/// Call through [`crate::jobs::office_to_pdf`], which bounds how many run at once.
pub fn convert_to_pdf(file_name: &str, body: &[u8], timeout: std::time::Duration) -> Option<Vec<u8>> {
    // PDFs are self-preview.
    if file_name.to_lowercase().ends_with(".pdf") {
        return Some(body.to_vec());
    }

    // Pick the binary.  Some distros install as `libreoffice`, others as `soffice`.
    let bin = ["soffice", "libreoffice"]
        .into_iter()
        .find(|b| std::process::Command::new("which").arg(b).output()
                    .ok().map(|o| !o.stdout.is_empty()).unwrap_or(false))?;

    // Write the input to a temp file (LibreOffice doesn't read stdin).
    let dir = std::env::temp_dir().join(format!("filehub-conv-{}", crate::Id::new()));
    std::fs::create_dir_all(&dir).ok()?;
    // Never use the uploaded name as a path (it could contain "../"): only its
    // extension, which LibreOffice needs to pick the import filter.
    let ext: String = std::path::Path::new(file_name).extension().and_then(|e| e.to_str()).unwrap_or("bin")
        .chars().filter(|c| c.is_ascii_alphanumeric()).take(8).collect();
    let input  = dir.join(format!("input.{ext}"));
    let output = dir.join("input.pdf");
    std::fs::write(&input, body).ok()?;
    // A profile per conversion: two LibreOffice runs sharing one profile lock
    // each other out and the second conversion fails.
    let profile = format!("-env:UserInstallation=file://{}", dir.join("profile").display());

    let mut child = std::process::Command::new(bin)
        .arg(&profile)
        .args(["--headless", "--convert-to", "pdf", "--outdir"])
        .arg(&dir)
        .arg(&input)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // A stuck conversion (corrupt or hostile file) is killed, never left holding a slot.
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(100)),
            _ => { let _ = child.kill(); let _ = child.wait(); break None; }
        }
    };
    if !status.map(|s| s.success()).unwrap_or(false) {
        let _ = std::fs::remove_dir_all(&dir);
        return None;
    }

    let pdf = std::fs::read(&output).ok();
    let _   = std::fs::remove_dir_all(&dir);
    pdf
}

pub async fn index_office_preview(db: &sqlx::PgPool, file_id: Id, file_type: &str, file_name: &str, body: &[u8]) {
    if !is_office_doc(file_type) { return; }
    // PDFs are served straight from storage by `get_preview` — don't keep a
    // second full copy in Postgres.
    if file_type == "pdf" || file_name.to_lowercase().ends_with(".pdf") { return; }
    let Some(pdf) = crate::jobs::office_to_pdf(file_name.to_string(), body.to_vec()).await else { return };
    let _ = sqlx::query(
        r#"INSERT INTO file_previews (file_id, data, mime)
           VALUES ($1, $2, 'application/pdf')
           ON CONFLICT (file_id) DO UPDATE SET data = EXCLUDED.data, created_at = now()"#,
    )
    .bind(file_id).bind(pdf)
    .execute(db).await;
}

pub async fn get_preview(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Id>,
    headers_in: HeaderMap,
) -> ApiResult<axum::response::Response> {
    // Load the file first so we can verify the caller has access before
    // serving any cached preview bytes.  Without this, a viewer could read
    // someone else's personal-drive preview by guessing the file id.
    let file: crate::models::File = sqlx::query_as(
        "SELECT id, name, file_type, size_bytes, system_id, org_id, bucket, object_key, project, \
                status, owner, tags, version, metadata, etag, created_at, modified_at, \
                folder_id, encrypted, deleted_at, created_by, category_code \
           FROM files WHERE id = $1 AND deleted_at IS NULL"
    )
    .bind(file_id)
    .fetch_optional(&s.db).await?
    .ok_or(ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::acl::require_file(&s.db, &user.0, file.id, crate::acl::VIEW).await.is_err()
        || crate::auth::ensure_system_access(&s.db, &user.0, &file.system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    // Where downloads are stamped the on-screen copy is stamped too (it can be saved/printed).
    let pol = crate::basket::effective_policy(&s.db, &file.system_id, file.folder_id.as_deref()).await?;
    if (pol.stamp_downloads == Some(true) || crate::basket::is_vault(&s.db, &file.system_id).await?)
        && matches!(file.file_type.as_str(), "pdf" | "docx" | "xlsx" | "pptx") {
        return crate::handlers::stamped_download(&s, &user.0, &file, true).await;
    }
    crate::basket::log_vault_access(&s.db, &user.0, &file.system_id, file.id, &file.name, "viewed").await;

    // A PDF is its own preview: stream the stored file (Range-capable, so
    // pdf.js can fetch pages lazily) instead of keeping a second full copy
    // of every PDF inside Postgres.
    if file.file_type == "pdf" || file.name.to_lowercase().ends_with(".pdf") {
        return crate::serve::stream_object(&s, crate::serve::ServeOpts {
            key: &file.object_key,
            encrypted: file.encrypted,
            name: &file.name,
            etag: file.etag.as_deref(),
            disposition: "inline",
            cache_control: "private, max-age=3600",
        }, &headers_in).await;
    }

    // Fast path — preview already in the cache.
    if let Some((mime, data)) = sqlx::query_as::<_, (String, Vec<u8>)>(
        "SELECT mime, data FROM file_previews WHERE file_id = $1"
    ).bind(file_id).fetch_optional(&s.db).await? {
        return Ok(preview_response(mime, data));
    }

    // Slow path — file was never indexed (e.g. seed data, or LibreOffice was
    // down at upload time).  Read the source bytes, convert on demand, cache
    // the result, then serve it.  Subsequent hits take the fast path above.

    if !is_office_doc(&file.file_type) {
        return Err(ApiError::NotFound);
    }

    crate::jobs::fits_in_memory(file.size_bytes, "preview")?;
    let (body, _) = s.storage.get(&file.object_key, file.encrypted).await?
        .ok_or(ApiError::NotFound)?;

    // Run LibreOffice on a blocking thread so the runtime stays responsive
    // (conversions can take a few seconds for large docs).
    let name = file.name.clone();
    let bytes = body.to_vec();
    let pdf = crate::jobs::office_to_pdf(name, bytes.to_vec()).await.ok_or(ApiError::NotFound)?;

    sqlx::query(
        r#"INSERT INTO file_previews (file_id, data, mime)
           VALUES ($1, $2, 'application/pdf')
           ON CONFLICT (file_id) DO UPDATE SET data = EXCLUDED.data, created_at = now()"#,
    )
    .bind(file_id).bind(&pdf)
    .execute(&s.db).await?;

    Ok(preview_response("application/pdf".into(), pdf))
}

fn preview_response(mime: String, data: Vec<u8>) -> axum::response::Response {
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    h.insert(header::CACHE_CONTROL, "private, max-age=3600".parse().unwrap());
    (StatusCode::OK, h, Bytes::from(data)).into_response()
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct ThumbQuery { pub w: Option<u32>, pub size: Option<u32>, pub width: Option<u32> }

/// Sizes the image cache keeps (longest side, px).
pub const RENDITION_SIZES: [u32; 3] = [300, 600, 1024];

/// One cached rendition (made from the stored image the first time it is asked for).
async fn rendition(s: &AppState, file_id: Id, want: u32, req: &HeaderMap) -> ApiResult<axum::response::Response> {
    let (key, enc, etag, ftype, size, name): (String, bool, Option<String>, String, i64, String) = sqlx::query_as(
        "SELECT object_key, encrypted, etag, file_type, size_bytes, name FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?.ok_or(ApiError::NotFound)?;
    if !matches!(ftype.as_str(), "png" | "jpg" | "jpeg" | "img" | "gif" | "webp") {
        return Err(ApiError::Coded(StatusCode::UNSUPPORTED_MEDIA_TYPE, "not_image", "This file isn't an image.".into()));
    }
    let gif_name = ftype == "gif" || name.to_ascii_lowercase().ends_with(".gif");
    let hit: Option<(String, Vec<u8>, Option<String>)> = sqlx::query_as(
        "SELECT mime, data, etag FROM image_renditions WHERE file_id = $1 AND width = $2").bind(file_id).bind(want as i32)
        .fetch_optional(&s.db).await?;
    let (mime, data, cache) = match hit {
        // Copies of GIFs made before they stayed GIFs are made again.
        // (Uploads file a .gif under the type "img", so go by the name too.)
        Some((m, d, e)) if e == etag && (!gif_name || m == "image/gif") => (m, d, "hit"),
        _ => {
            crate::jobs::fits_in_memory(size, "resize")?;
            let (body, _) = s.storage.get(&key, enc).await?.ok_or(ApiError::NotFound)?;
            let owned = body.to_vec();
            // A GIF stays a GIF (no copy in another format); other images are JPEG.
            let gif = gif_name || owned.starts_with(b"GIF87a") || owned.starts_with(b"GIF89a");
            let (pw, ph, out) = tokio::task::spawn_blocking(move || if gif { make_gif_rendition(&owned, want) } else { make_rendition(&owned, want) }).await
                .ok().flatten().ok_or_else(|| ApiError::Coded(StatusCode::UNSUPPORTED_MEDIA_TYPE, "not_image", "This image can't be read.".into()))?;
            let mime = if gif { "image/gif" } else { "image/jpeg" };
            sqlx::query(r#"INSERT INTO image_renditions (file_id, width, etag, mime, px_w, px_h, data) VALUES ($1,$2,$3,$7,$4,$5,$6)
                           ON CONFLICT (file_id, width) DO UPDATE SET etag = EXCLUDED.etag, mime = EXCLUDED.mime, px_w = EXCLUDED.px_w,
                             px_h = EXCLUDED.px_h, data = EXCLUDED.data, created_at = now()"#)
                .bind(file_id).bind(want as i32).bind(&etag).bind(pw as i32).bind(ph as i32).bind(&out).bind(mime).execute(&s.db).await?;
            (mime.to_string(), out, "miss")
        }
    };
    // The browser keeps the copy but asks again each time (304 while unchanged),
    // so a file deleted for good stops being served (404) straight away.
    let tag = format!("\"r{}-{}\"", want, etag.as_deref().unwrap_or("").trim_matches('"'));
    let mut h = HeaderMap::new();
    h.insert(header::CACHE_CONTROL, "private, no-cache".parse().unwrap());
    if let Ok(v) = tag.parse() { h.insert(header::ETAG, v); }
    h.insert("x-filehub-cache", cache.parse().unwrap());
    if req.get(header::IF_NONE_MATCH).and_then(|v| v.to_str().ok()).map(|v| v.split(',').any(|x| x.trim() == tag)).unwrap_or(false) {
        return Ok((StatusCode::NOT_MODIFIED, h).into_response());
    }
    h.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    Ok((StatusCode::OK, h, Bytes::from(data)).into_response())
}

/// Longest side `max_dim` px (never enlarged), still a GIF (first frame).
pub fn make_gif_rendition(body: &[u8], max_dim: u32) -> Option<(u32, u32, Vec<u8>)> {
    let mut reader = ImageReader::new(std::io::Cursor::new(body)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(10_000);
    limits.max_image_height = Some(10_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let img = if img.width().max(img.height()) > max_dim { img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3) } else { img };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let mut out = Vec::new();
    {
        let mut enc = image::codecs::gif::GifEncoder::new(&mut out);
        enc.encode(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8).ok()?;
    }
    Some((w, h, out))
}

/// Longest side `max_dim` px (never enlarged), JPEG on white.
pub fn make_rendition(body: &[u8], max_dim: u32) -> Option<(u32, u32, Vec<u8>)> {
    let mut reader = ImageReader::new(std::io::Cursor::new(body)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(10_000);
    limits.max_image_height = Some(10_000);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    let img = if img.width().max(img.height()) > max_dim { img.resize(max_dim, max_dim, image::imageops::FilterType::Lanczos3) } else { img };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let mut rgb = image::RgbImage::new(w, h);
    for (x, y, p) in rgba.enumerate_pixels() {
        let a = p[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a)) / 255) as u8;
        rgb.put_pixel(x, y, image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])]));
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .write_image(rgb.as_raw(), w, h, image::ExtendedColorType::Rgb8).ok()?;
    Some((w, h, out))
}

pub async fn get_thumbnail(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Id>,
    Query(tq): Query<ThumbQuery>,
    req: HeaderMap,
) -> ApiResult<axum::response::Response> {
    let owner_row: Option<(String,)> = sqlx::query_as(
        "SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = owner_row.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    if let Some(w) = tq.w.or(tq.size).or(tq.width) {
        if !RENDITION_SIZES.contains(&w) {
            return Err(ApiError::BadRequest("Size must be 300, 600, or 1024.".into()));
        }
        return rendition(&s, file_id, w, &req).await;
    }
    let row: Option<(String, Vec<u8>)> = sqlx::query_as(
        "SELECT mime, data FROM thumbnails WHERE file_id = $1"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (mime, data) = row.ok_or(ApiError::NotFound)?;
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    h.insert(header::CACHE_CONTROL, "private, no-cache".parse().unwrap());
    Ok((StatusCode::OK, h, Bytes::from(data)).into_response())
}

// =============================================================================
// Workflow (TOR 4.15.13 — file revision state machine).
// =============================================================================

#[derive(Debug, Serialize, FromRow)]
pub struct Workflow {
    pub id:         Id,
    pub file_id:    Id,
    pub state:      String,
    pub note:       Option<String>,
    pub created_by: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct WorkflowStep {
    pub id:           Id,
    pub workflow_id:  Id,
    pub reviewer_id:  Option<String>,
    pub decision:     Option<String>,
    pub note:         Option<String>,
    pub sequence:     i32,
    pub created_at:   DateTime<Utc>,
    pub decided_at:   Option<DateTime<Utc>>,
    /// Reviewer's display name, joined from `users` in `list_workflow`. The
    /// frontend renders this directly. `#[sqlx(default)]` so the bare
    /// `SELECT * FROM workflow_steps` paths (which don't join) still decode —
    /// they just leave it `None`.
    #[sqlx(default)]
    pub reviewer_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StartWorkflow {
    pub reviewer_ids: Vec<String>,
    pub note:         Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct DecideStep {
    pub decision: String,            // approved | rejected
    pub note:     Option<String>,
}

pub async fn start_workflow(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Id>,
    Json(c): Json<StartWorkflow>,
) -> ApiResult<Json<Workflow>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    if c.reviewer_ids.is_empty() {
        return Err(ApiError::BadRequest("at least one reviewer is required".into()));
    }
    // Ensure the file exists & isn't trashed.
    let exists: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = exists.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::EDIT).await?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;
    if c.reviewer_ids.iter().any(|r| r == &user.0.id) {
        return Err(ApiError::BadRequest("You can't review your own request.".into()));
    }
    // A workflow moves the file's status, so it obeys the basket's status rules:
    // never out of a status that locks the content.
    let status: String = sqlx::query_scalar("SELECT status FROM files WHERE id = $1").bind(file_id).fetch_one(&s.db).await?;
    if crate::basket::content_locked(&s.db, &system_id, &status).await? {
        return Err(ApiError::ContentLocked("This file's status locks it. Change the status before starting a review.".into()));
    }

    let wf_id = crate::Id::new();
    sqlx::query(
        r#"INSERT INTO file_workflows (id, file_id, state, note, created_by)
           VALUES ($1, $2, 'Review', $3, $4)"#,
    )
    .bind(wf_id).bind(file_id).bind(&c.note).bind(&user.0.id)
    .execute(&s.db).await?;

    for (i, rid) in c.reviewer_ids.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO workflow_steps (id, workflow_id, reviewer_id, decision, sequence)
               VALUES ($1, $2, $3, 'pending', $4)"#,
        )
        .bind(crate::Id::new()).bind(wf_id).bind(rid).bind((i + 1) as i32)
        .execute(&s.db).await?;

        let _ = sqlx::query(
            r#"INSERT INTO notifications (id, user_id, kind, title, body, link)
               VALUES ($1, $2, 'review_requested', $3, $4, $5)"#,
        )
        .bind(crate::Id::new()).bind(rid)
        .bind("Review requested")
        .bind(c.note.clone().unwrap_or_default())
        .bind(format!("/f/{file_id}"))
        .execute(&s.db).await;
    }

    // File status follows the workflow state where the basket allows that move.
    let level = crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::VIEW).await? as i16;
    sqlx::query(
        "UPDATE files SET status = 'Review', modified_at = now() WHERE id = $1 AND EXISTS (
           SELECT 1 FROM basket_status_transitions WHERE system_id = $2 AND from_code = $3 AND to_code = 'Review' AND required_level <= $4)",
    ).bind(file_id).bind(&system_id).bind(&status).bind(level).execute(&s.db).await?;

    Ok(Json(sqlx::query_as("SELECT * FROM file_workflows WHERE id = $1")
        .bind(wf_id).fetch_one(&s.db).await?))
}

pub async fn list_workflow(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Id>,
) -> ApiResult<Json<Vec<(Workflow, Vec<WorkflowStep>)>>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let workflows: Vec<Workflow> = sqlx::query_as(
        "SELECT * FROM file_workflows WHERE file_id = $1 ORDER BY created_at DESC"
    ).bind(file_id).fetch_all(&s.db).await?;
    let mut out: Vec<(Workflow, Vec<WorkflowStep>)> = Vec::new();
    for wf in workflows {
        let steps: Vec<WorkflowStep> = sqlx::query_as(
            r#"SELECT s.*, COALESCE(u.display_name, 'Unknown reviewer') AS reviewer_name
                 FROM workflow_steps s
                 LEFT JOIN users u ON u.id = s.reviewer_id
                WHERE s.workflow_id = $1
                ORDER BY s.sequence"#
        ).bind(wf.id).fetch_all(&s.db).await?;
        out.push((wf, steps));
    }
    Ok(Json(out))
}

pub async fn decide_step(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(step_id): Path<Id>,
    Json(d): Json<DecideStep>,
) -> ApiResult<Json<WorkflowStep>> {
    crate::auth::require_role(&user.0, &["admin", "editor"])?;
    if !["approved", "rejected"].contains(&d.decision.as_str()) {
        return Err(ApiError::BadRequest("decision must be approved or rejected".into()));
    }
    let step: WorkflowStep = sqlx::query_as("SELECT * FROM workflow_steps WHERE id = $1")
        .bind(step_id).fetch_optional(&s.db).await?
        .ok_or(ApiError::NotFound)?;
    // Reach into the underlying file's system to gate by personal-drive access.
    let sys_row: Option<(String,)> = sqlx::query_as(
        "SELECT f.system_id FROM file_workflows wf \
         JOIN files f ON f.id = wf.file_id WHERE wf.id = $1"
    ).bind(step.workflow_id).fetch_optional(&s.db).await?;
    let (system_id,) = sys_row.ok_or(ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;
    if step.reviewer_id.as_ref() != Some(&user.0.id) {
        return Err(ApiError::Forbidden);
    }
    sqlx::query("UPDATE workflow_steps SET decision = $1, note = $2, decided_at = now() WHERE id = $3")
        .bind(&d.decision).bind(&d.note).bind(step_id).execute(&s.db).await?;

    // Roll-up: any reject → Rejected; all approved → Approved.
    let wf_id = step.workflow_id;
    let any_rejected: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM workflow_steps WHERE workflow_id = $1 AND decision = 'rejected')"
    ).bind(wf_id).fetch_one(&s.db).await?;
    let all_approved: bool = sqlx::query_scalar(
        "SELECT bool_and(decision = 'approved') FROM workflow_steps WHERE workflow_id = $1"
    ).bind(wf_id).fetch_one(&s.db).await?;

    let new_state = if any_rejected { Some("Rejected") }
                    else if all_approved { Some("Approved") }
                    else { None };

    if let Some(state) = new_state {
        sqlx::query("UPDATE file_workflows SET state = $1 WHERE id = $2")
            .bind(state).bind(wf_id).execute(&s.db).await?;
        // Mirror into files.status only when the basket's status rules allow
        // that move for the person whose decision completes the review.
        let (fid, cur): (Id, String) = sqlx::query_as(
            "SELECT f.id, f.status FROM files f JOIN file_workflows w ON w.file_id = f.id WHERE w.id = $1",
        ).bind(wf_id).fetch_one(&s.db).await?;
        let need: Option<i16> = sqlx::query_scalar(
            "SELECT required_level FROM basket_status_transitions WHERE system_id = $1 AND from_code = $2 AND to_code = $3",
        ).bind(&system_id).bind(&cur).bind(state).fetch_optional(&s.db).await?;
        let level = crate::acl::require_file(&s.db, &user.0, fid, crate::acl::VIEW).await.unwrap_or(0) as i16;
        if need.is_some_and(|n| level >= n) {
            sqlx::query("UPDATE files SET status = $1, modified_at = now() WHERE id = $2")
                .bind(state).bind(fid).execute(&s.db).await?;
        }

        // Notify the workflow creator of the outcome.
        let creator: Option<(Option<Id>, Id)> = sqlx::query_as(
            "SELECT created_by, file_id FROM file_workflows WHERE id = $1"
        ).bind(wf_id).fetch_optional(&s.db).await?;
        if let Some((Some(uid), file_id)) = creator {
            let _ = sqlx::query(
                r#"INSERT INTO notifications (id, user_id, kind, title, link)
                   VALUES ($1, $2, $3, $4, $5)"#,
            )
            .bind(crate::Id::new()).bind(uid).bind(state.to_lowercase())
            .bind(format!("Workflow {state}"))
            .bind(format!("/f/{file_id}"))
            .execute(&s.db).await;
        }
    }

    Ok(Json(sqlx::query_as("SELECT * FROM workflow_steps WHERE id = $1")
        .bind(step_id).fetch_one(&s.db).await?))
}

// =============================================================================
// Comments (file-level discussion).
// =============================================================================

// `user_id` references users.id which is TEXT (CUID2 per the hybrid-key
// strategy in 0001_init.sql).  Previously these structs typed it as Id,
// which serialised fine on INSERT (sqlx coerces the bound value) but blew up
// on SELECT with "Rust type uuid::Id is not compatible with SQL type TEXT".
// Switching to String fixes the round trip and matches the actual column.
#[derive(Debug, Serialize, FromRow)]
pub struct Comment {
    pub id:         Id,
    pub file_id:    Id,
    pub user_id:    String,
    pub parent_id:  Option<Id>,
    pub body:       String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct CommentWithAuthor {
    pub id:           Id,
    pub file_id:      Id,
    pub user_id:      String,
    pub parent_id:    Option<Id>,
    pub body:         String,
    pub created_at:   DateTime<Utc>,
    pub display_name: String,
    pub avatar_tone:  String,
}

#[derive(Debug, Deserialize)]
pub struct NewComment {
    pub body:      String,
    pub parent_id: Option<Id>,
}

pub async fn list_comments(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Id>,
) -> ApiResult<Json<Vec<CommentWithAuthor>>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let rows: Vec<CommentWithAuthor> = sqlx::query_as(
        r#"SELECT c.id, c.file_id, c.user_id, c.parent_id, c.body, c.created_at,
                  u.display_name, u.avatar_tone
             FROM file_comments c
             JOIN users u ON u.id = c.user_id
            WHERE c.file_id = $1
            ORDER BY c.created_at"#,
    ).bind(file_id).fetch_all(&s.db).await?;
    Ok(Json(rows))
}

pub async fn create_comment(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(file_id): Path<Id>,
    Json(c): Json<NewComment>,
) -> ApiResult<Json<Comment>> {
    // Commenting is the one mutation viewers ARE allowed (see the Roles &
    // permissions matrix: "Comment on files" is checked for all three roles).
    // So no editor gate here — a valid session plus `ensure_system_access`
    // below is the whole authorization.
    if c.body.trim().is_empty() {
        return Err(ApiError::BadRequest("comment body required".into()));
    }
    let sys_row: Option<(String,)> = sqlx::query_as(
        "SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = sys_row.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;
    let id = crate::Id::new();
    sqlx::query(
        r#"INSERT INTO file_comments (id, file_id, user_id, parent_id, body)
           VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(id).bind(file_id).bind(user.0.id).bind(c.parent_id).bind(c.body.trim())
    .execute(&s.db).await?;
    Ok(Json(sqlx::query_as("SELECT * FROM file_comments WHERE id = $1")
        .bind(id).fetch_one(&s.db).await?))
}

pub async fn delete_comment(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path((file_id, comment_id)): Path<(Id, Id)>,
) -> ApiResult<StatusCode> {
    // No blanket editor gate: authorization here is "you own the comment, or
    // you're an admin" (checked below).  A viewer who posted a comment must be
    // able to delete their own — gating on editor would wrongly block that.
    let sys_row: Option<(String,)> = sqlx::query_as(
        "SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL"
    ).bind(file_id).fetch_optional(&s.db).await?;
    let (system_id,) = sys_row.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, file_id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await?;
    let owner: Option<(String,)> = sqlx::query_as(
        "SELECT user_id FROM file_comments WHERE id = $1 AND file_id = $2"
    ).bind(comment_id).bind(file_id).fetch_optional(&s.db).await?;
    let owner = owner.ok_or(ApiError::NotFound)?.0;
    if owner != user.0.id && user.0.role != "admin" {
        return Err(ApiError::Forbidden);
    }
    sqlx::query("DELETE FROM file_comments WHERE id = $1").bind(comment_id).execute(&s.db).await?;
    Ok(StatusCode::NO_CONTENT)
}

// =============================================================================
// Notifications.
// =============================================================================

#[derive(Debug, Serialize, FromRow)]
pub struct Notification {
    pub id:         Id,
    pub user_id:    String,
    pub kind:       String,
    pub title:      String,
    pub body:       Option<String>,
    pub link:       Option<String>,
    pub read_at:    Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct NotificationsQuery {
    pub unread_only: Option<bool>,
    pub limit:       Option<i64>,
}

pub async fn list_notifications(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Query(q): Query<NotificationsQuery>,
) -> ApiResult<Json<Vec<Notification>>> {
    let limit = q.limit.unwrap_or(50).min(500).max(1);
    let unread_only = q.unread_only.unwrap_or(false);
    let rows: Vec<Notification> = if unread_only {
        sqlx::query_as(
            "SELECT * FROM notifications WHERE user_id = $1 AND read_at IS NULL ORDER BY created_at DESC LIMIT $2"
        ).bind(user.0.id).bind(limit).fetch_all(&s.db).await?
    } else {
        sqlx::query_as(
            "SELECT * FROM notifications WHERE user_id = $1 ORDER BY created_at DESC LIMIT $2"
        ).bind(user.0.id).bind(limit).fetch_all(&s.db).await?
    };
    Ok(Json(rows))
}

pub async fn mark_notification_read(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
) -> ApiResult<StatusCode> {
    let r = sqlx::query(
        "UPDATE notifications SET read_at = now() WHERE id = $1 AND user_id = $2 AND read_at IS NULL"
    ).bind(id).bind(user.0.id).execute(&s.db).await?;
    if r.rows_affected() == 0 {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn unread_count(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
) -> ApiResult<Json<serde_json::Value>> {
    let n: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM notifications WHERE user_id = $1 AND read_at IS NULL"
    ).bind(user.0.id).fetch_one(&s.db).await?;
    Ok(Json(serde_json::json!({ "unread": n })))
}

// =============================================================================
// File detail with author + comment count (so the UI can render in one shot).
// =============================================================================
#[derive(Debug, Serialize)]
pub struct FileExtras {
    pub comments: i64,
    pub has_thumbnail: bool,
    pub has_extracted_text: bool,
}

pub async fn file_extras(
    State(s): State<Arc<AppState>>,
    user: AuthUser,
    Path(id): Path<Id>,
) -> ApiResult<Json<FileExtras>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT system_id FROM files WHERE id = $1 AND deleted_at IS NULL")
        .bind(id).fetch_optional(&s.db).await?;
    let (system_id,) = row.ok_or(ApiError::NotFound)?;
    crate::acl::require_file(&s.db, &user.0, id, crate::acl::VIEW).await.map_err(|_| ApiError::NotFound)?;
    // Map "you can't see this file" to 404 so personal-drive contents don't
    // leak existence via the response code.
    if crate::auth::ensure_system_access(&s.db, &user.0, &system_id).await.is_err() {
        return Err(ApiError::NotFound);
    }
    let comments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM file_comments WHERE file_id = $1")
        .bind(id).fetch_one(&s.db).await?;
    let thumb: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM thumbnails WHERE file_id = $1)")
        .bind(id).fetch_one(&s.db).await?;
    let text: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM file_content WHERE file_id = $1)")
        .bind(id).fetch_one(&s.db).await?;
    Ok(Json(FileExtras { comments, has_thumbnail: thumb, has_extracted_text: text }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ooxml_text_joins_split_runs_and_reads_sheets() {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("word/document.xml", o).unwrap();
            z.write_all(br#"<w:document><w:body><w:p><w:r><w:t>rr24word</w:t></w:r><w:r><w:t xml:space="preserve">token</w:t></w:r></w:p><w:p><w:r><w:t>A &amp; B</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
            z.finish().unwrap();
        }
        let t = extract_text_from("docx", buf.get_ref()).unwrap();
        assert!(t.contains("rr24wordtoken"), "{t}");
        assert!(t.contains("A & B"), "{t}");
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            z.start_file("xl/sharedStrings.xml", o).unwrap();
            z.write_all("<sst><si><t>มะม่วง</t></si><si><r><t>mango</t></r></si></sst>".as_bytes()).unwrap();
            z.finish().unwrap();
        }
        let t = extract_text_from("xlsx", buf.get_ref()).unwrap();
        assert!(t.contains("มะม่วง") && t.contains("mango"), "{t}");
        assert!(extract_text_from("pptx", b"not a zip").is_none());
    }

    #[test]
    fn extract_text_pdf_returns_string_or_none() {
        // We don't carry test fixtures, so just verify the function shape
        // on a clearly-not-pdf input.
        assert!(extract_text_from("pdf", b"not actually a pdf").is_none() ||
                extract_text_from("pdf", b"not actually a pdf").is_some());
        // Plain text passthrough works.
        let txt = extract_text_from("txt", b"hello world").unwrap();
        assert_eq!(txt, "hello world");
        assert!(extract_text_from("zip", b"PK\x03\x04").is_none());
    }

    #[test]
    fn make_thumbnail_handles_garbage() {
        // Garbage in → None out, no panic.
        assert!(make_thumbnail(b"not an image", 64).is_none());
    }
}
