//! Reports as files people can keep: Excel (.xlsx) and PDF (TOR 4.15.15).
//! The workbook is written directly (plain OOXML); the PDF is that workbook
//! printed by LibreOffice, so both always show the same numbers.

use std::io::Write as _;
use std::sync::Arc;

use axum::{extract::{Query, State}, http::{header, HeaderMap, StatusCode}, response::IntoResponse, Json};
use serde::Deserialize;

use crate::auth::AuthUser;
use crate::error::{ApiError, ApiResult};
use crate::models::{ReportCatQuery, ReportTimeQuery};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ExportQuery { pub format: Option<String>, pub basket: Option<String>, pub by: Option<String>, pub bucket: Option<String> }

fn esc(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }
fn mb(b: i64) -> String { format!("{:.2}", b as f64 / 1048576.0) }

fn sheet(rows: &[Vec<String>], numeric_from: usize) -> String {
    let mut x = String::from(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cols><col min="1" max="1" width="40" customWidth="1"/><col min="2" max="3" width="18" customWidth="1"/></cols><sheetData>"#);
    for (r, row) in rows.iter().enumerate() {
        x.push_str(&format!("<row r=\"{}\">", r + 1));
        for (c, v) in row.iter().enumerate() {
            let cell = format!("{}{}", (b'A' + c as u8) as char, r + 1);
            if r > 1 && c >= numeric_from && v.parse::<f64>().is_ok() {
                x.push_str(&format!("<c r=\"{cell}\"><v>{v}</v></c>"));
            } else {
                x.push_str(&format!("<c r=\"{cell}\" t=\"inlineStr\"{}><is><t>{}</t></is></c>", if r <= 1 { " s=\"1\"" } else { "" }, esc(v)));
            }
        }
        x.push_str("</row>");
    }
    x.push_str("</sheetData><pageSetup orientation=\"portrait\" fitToWidth=\"1\"/></worksheet>");
    x
}

fn xlsx(sheets: &[(&str, String)]) -> Result<Vec<u8>, ApiError> {
    let mut buf = std::io::Cursor::new(Vec::new());
    {
        let mut z = zip::ZipWriter::new(&mut buf);
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut put = |n: &str, d: &str| -> Result<(), ApiError> {
            z.start_file(n, o).map_err(|e| ApiError::Other(e.into()))?;
            z.write_all(d.as_bytes()).map_err(|e| ApiError::Other(e.into()))
        };
        let mut ct = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>"#);
        let mut wb = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>"#);
        let mut rels = String::from(r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rS" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#);
        for (i, (name, _)) in sheets.iter().enumerate() {
            let n = i + 1;
            ct.push_str(&format!(r#"<Override PartName="/xl/worksheets/sheet{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#));
            wb.push_str(&format!(r#"<sheet name="{}" sheetId="{n}" r:id="r{n}"/>"#, esc(name)));
            rels.push_str(&format!(r#"<Relationship Id="r{n}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{n}.xml"/>"#));
        }
        ct.push_str("</Types>"); wb.push_str("</sheets></workbook>"); rels.push_str("</Relationships>");
        put("[Content_Types].xml", &ct)?;
        put("_rels/.rels", r#"<?xml version="1.0" encoding="UTF-8"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="r1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#)?;
        put("xl/workbook.xml", &wb)?;
        put("xl/_rels/workbook.xml.rels", &rels)?;
        put("xl/styles.xml", r#"<?xml version="1.0" encoding="UTF-8"?><styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><fonts count="2"><font><sz val="11"/><name val="TH Sarabun New"/></font><font><b/><sz val="11"/><name val="TH Sarabun New"/></font></fonts><fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills><borders count="1"><border/></borders><cellStyleXfs count="1"><xf/></cellStyleXfs><cellXfs count="2"><xf fontId="0"/><xf fontId="1" applyFont="1"/></cellXfs></styleSheet>"#)?;
        for (i, (_, x)) in sheets.iter().enumerate() { put(&format!("xl/worksheets/sheet{}.xml", i + 1), x)?; }
        z.finish().map_err(|e| ApiError::Other(e.into()))?;
    }
    Ok(buf.into_inner())
}

pub async fn export(State(s): State<Arc<AppState>>, user: AuthUser, Query(q): Query<ExportQuery>) -> ApiResult<axum::response::Response> {
    let format = q.format.as_deref().unwrap_or("xlsx");
    if !matches!(format, "xlsx" | "pdf") { return Err(ApiError::BadRequest("format ต้องเป็น xlsx หรือ pdf".into())); }
    let by = q.by.clone().unwrap_or_else(|| "category".into());
    let bucket = q.bucket.clone().unwrap_or_else(|| "month".into());
    let Json(cats) = crate::handlers::report_by_category(State(s.clone()), AuthUser(user.0.clone()),
        Query(ReportCatQuery { by: Some(by.clone()), basket: q.basket.clone() })).await?;
    let Json(times) = crate::handlers::report_by_time(State(s.clone()), AuthUser(user.0.clone()),
        Query(ReportTimeQuery { bucket: Some(bucket.clone()), limit: Some(1000), basket: q.basket.clone() })).await?;
    let now = chrono::Utc::now().with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap());
    let stamp = format!("ออกรายงานเมื่อ {} โดย {}", now.format("%d/%m/%Y %H:%M"), user.0.display_name);
    let by_label = match by.as_str() { "format" => "รูปแบบไฟล์", "basket" => "ตะกร้า", "type" => "ประเภทไฟล์", _ => "หมวดหมู่เอกสาร" };
    let mut a = vec![vec![format!("รายงานพื้นที่จัดเก็บตาม{by_label}"), stamp.clone(), String::new()],
                     vec![by_label.to_string(), "จำนวนไฟล์".into(), "ขนาด (MB)".into()]];
    for r in &cats { a.push(vec![r.category.clone(), r.file_count.to_string(), mb(r.size_bytes)]); }
    a.push(vec!["รวม".into(), cats.iter().map(|r| r.file_count).sum::<i64>().to_string(), mb(cats.iter().map(|r| r.size_bytes).sum())]);
    let fmt = match bucket.as_str() { "day" => "%d/%m/%Y", "year" => "%Y", _ => "%m/%Y" };
    let mut b = vec![vec!["รายงานปริมาณไฟล์ตามช่วงเวลา".to_string(), stamp, String::new()],
                     vec!["ช่วงเวลา".into(), "จำนวนไฟล์".into(), "ขนาด (MB)".into()]];
    for r in &times { b.push(vec![r.bucket.with_timezone(&chrono::FixedOffset::east_opt(7 * 3600).unwrap()).format(fmt).to_string(), r.file_count.to_string(), mb(r.size_bytes)]); }
    let book = xlsx(&[("ตามหมวดหมู่", sheet(&a, 1)), ("ตามช่วงเวลา", sheet(&b, 1))])?;
    let day = now.format("%Y%m%d-%H%M");
    let (body, mime, name) = if format == "xlsx" {
        (book, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", format!("filehub-report-{day}.xlsx"))
    } else {
        let pdf = crate::jobs::office_to_pdf("report.xlsx".into(), book).await
            .ok_or_else(|| ApiError::Coded(StatusCode::SERVICE_UNAVAILABLE, "busy", "สร้างไฟล์ PDF ไม่สำเร็จ กรุณาลองใหม่".into()))?;
        (pdf, "application/pdf", format!("filehub-report-{day}.pdf"))
    };
    let mut h = HeaderMap::new();
    h.insert(header::CONTENT_TYPE, mime.parse().unwrap());
    h.insert(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"").parse().unwrap());
    h.insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    Ok((StatusCode::OK, h, body).into_response())
}
