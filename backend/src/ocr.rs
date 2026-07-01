//! Pluggable OCR — turns scanned images and image-only PDFs into searchable
//! Thai+English text (TOR Annex A: OCR full-text search over scanned files).
//!
//! Local-first and optional: shells out to **Tesseract** (`-l tha+eng` by
//! default) for images, and to **pdftoppm** (Poppler) to rasterise scanned PDFs
//! first. If the tools aren't installed it cleanly returns `None`, so a build
//! without OCR still works — OCR just doesn't add anything until the binaries
//! are present. Everything is best-effort and never panics.
//!
//! Config (env): `OCR_ENABLED` (default true), `OCR_LANGS` (default `tha+eng`),
//! `OCR_TESSERACT_CMD`, `OCR_PDFTOPPM_CMD`, `OCR_MAX_PDF_PAGES` (default 30).

use std::path::{Path, PathBuf};

use tokio::process::Command;
use uuid::Uuid;

const IMG_TYPES: &[&str] = &["png", "jpg", "jpeg", "tiff", "tif", "bmp", "gif", "img"];

pub fn enabled() -> bool {
    !matches!(
        std::env::var("OCR_ENABLED").ok().as_deref(),
        Some("0") | Some("false") | Some("no") | Some("off")
    )
}

fn tesseract_cmd() -> String {
    std::env::var("OCR_TESSERACT_CMD").unwrap_or_else(|_| "tesseract".into())
}
fn pdftoppm_cmd() -> String {
    std::env::var("OCR_PDFTOPPM_CMD").unwrap_or_else(|_| "pdftoppm".into())
}
fn langs() -> String {
    std::env::var("OCR_LANGS").unwrap_or_else(|_| "tha+eng".into())
}
fn max_pdf_pages() -> usize {
    std::env::var("OCR_MAX_PDF_PAGES").ok().and_then(|v| v.parse().ok()).unwrap_or(30)
}

/// True for file types OCR can handle (raster images + PDF).
pub fn is_ocrable(file_type: &str) -> bool {
    let ft = file_type.to_ascii_lowercase();
    IMG_TYPES.contains(&ft.as_str()) || ft == "pdf"
}

fn tmp(ext: &str) -> PathBuf {
    std::env::temp_dir().join(format!("fh-ocr-{}.{ext}", Uuid::now_v7()))
}

/// OCR `body` (an image or PDF). Returns extracted text, or `None` if OCR is
/// disabled/unavailable, fails, or the document has no readable text.
pub async fn ocr_extract(file_type: &str, body: &[u8]) -> Option<String> {
    if !enabled() || body.is_empty() {
        return None;
    }
    let ft = file_type.to_ascii_lowercase();
    if ft == "pdf" {
        ocr_pdf(body).await
    } else if IMG_TYPES.contains(&ft.as_str()) {
        ocr_image(body).await
    } else {
        None
    }
}

async fn run_tesseract(path: &Path) -> Option<String> {
    let out = Command::new(tesseract_cmd())
        .arg(path)
        .arg("stdout")
        .arg("-l")
        .arg(langs())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if t.is_empty() { None } else { Some(t) }
}

async fn ocr_image(body: &[u8]) -> Option<String> {
    let inp = tmp("img");
    tokio::fs::write(&inp, body).await.ok()?;
    let text = run_tesseract(&inp).await;
    let _ = tokio::fs::remove_file(&inp).await;
    text
}

/// Rasterise the PDF to PNG pages (Poppler `pdftoppm`) then OCR each page in
/// order. Capped at `OCR_MAX_PDF_PAGES` so a huge scan can't run unbounded.
async fn ocr_pdf(body: &[u8]) -> Option<String> {
    let inp = tmp("pdf");
    tokio::fs::write(&inp, body).await.ok()?;
    let prefix = std::env::temp_dir().join(format!("fh-ocr-{}", Uuid::now_v7()));

    let render = Command::new(pdftoppm_cmd())
        .arg("-png")
        .arg("-r").arg("200")
        .arg("-l").arg(max_pdf_pages().to_string())
        .arg(&inp)
        .arg(&prefix)
        .output()
        .await;
    let _ = tokio::fs::remove_file(&inp).await;
    if !render.ok()?.status.success() {
        return None;
    }

    // pdftoppm writes <prefix>-1.png, <prefix>-2.png, … in temp_dir.
    let dir = std::env::temp_dir();
    let base = prefix.file_name()?.to_string_lossy().to_string();
    let mut pages: Vec<PathBuf> = Vec::new();
    let mut rd = tokio::fs::read_dir(&dir).await.ok()?;
    while let Ok(Some(e)) = rd.next_entry().await {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with(&base) && name.ends_with(".png") {
            pages.push(e.path());
        }
    }
    pages.sort();

    let mut acc = String::new();
    for p in &pages {
        if let Some(t) = run_tesseract(p).await {
            acc.push_str(&t);
            acc.push('\n');
        }
        let _ = tokio::fs::remove_file(p).await;
    }
    let acc = acc.trim().to_string();
    if acc.is_empty() { None } else { Some(acc) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ocrable_types() {
        for t in ["png", "jpg", "JPEG", "tiff", "pdf", "PDF"] {
            assert!(is_ocrable(t), "{t} should be OCR-able");
        }
        for t in ["txt", "docx", "mp4", "zip"] {
            assert!(!is_ocrable(t), "{t} should not be OCR-able");
        }
    }

    #[tokio::test]
    async fn empty_body_is_none() {
        assert!(ocr_extract("png", b"").await.is_none());
    }
}
