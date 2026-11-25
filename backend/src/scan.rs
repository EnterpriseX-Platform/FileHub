//! Checks on every uploaded file, done by FileHub itself so a WAF in front of
//! the upload endpoint doesn't have to inspect file bodies:
//!
//! - **Real type** — the first bytes must match the extension for formats we
//!   know (a `.pdf` must start `%PDF`, a `.docx` must be a ZIP, …), and
//!   programs (Windows/Linux/macOS executables) are refused unless the basket
//!   allows the extension explicitly.
//! - **Malware** — with `CLAMAV_HOST` set (host:port of clamd), bytes are
//!   streamed to clamd (INSTREAM) while they are written, and an infected file
//!   is refused. `CLAMAV_REQUIRED=true` refuses uploads while clamd can't be
//!   reached; otherwise they go through and a warning is logged.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Bytes kept from the start of a file for the type check.
pub const HEAD: usize = 8192;
/// clamd refuses streams larger than its StreamMaxLength (default 25 MB):
/// stop sending there and scan what was sent.
const CLAM_MAX: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    /// Not scanned (no clamd configured, or it couldn't be reached and isn't required).
    Skipped,
    Infected(String),
    /// clamd required but unreachable / failed.
    Unavailable(String),
}

/// Streams bytes to clamd while an upload is staged.
pub struct ClamStream {
    conn: Option<TcpStream>,
    sent: u64,
    err: Option<String>,
}

fn clam_host() -> Option<String> {
    std::env::var("CLAMAV_HOST").ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn clam_required() -> bool {
    matches!(std::env::var("CLAMAV_REQUIRED").unwrap_or_default().trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

impl ClamStream {
    /// `None` when no clamd is configured.
    pub async fn open() -> Option<Self> {
        let host = clam_host()?;
        let conn = match tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(&host)).await {
            Ok(Ok(mut c)) => match c.write_all(b"zINSTREAM\0").await {
                Ok(()) => Some(c),
                Err(e) => return Some(Self { conn: None, sent: 0, err: Some(format!("clamd: {e}")) }),
            },
            Ok(Err(e)) => return Some(Self { conn: None, sent: 0, err: Some(format!("clamd {host}: {e}")) }),
            Err(_) => return Some(Self { conn: None, sent: 0, err: Some(format!("clamd {host}: timed out")) }),
        };
        Some(Self { conn, sent: 0, err: None })
    }

    pub async fn write(&mut self, data: &[u8]) {
        if self.sent >= CLAM_MAX { return; }
        let Some(c) = self.conn.as_mut() else { return };
        let take = data.len().min((CLAM_MAX - self.sent) as usize);
        for part in data[..take].chunks(1 << 20) {
            let len = (part.len() as u32).to_be_bytes();
            if let Err(e) = async { c.write_all(&len).await?; c.write_all(part).await }.await {
                self.err = Some(format!("clamd: {e}"));
                self.conn = None;
                return;
            }
        }
        self.sent += take as u64;
    }

    pub async fn finish(mut self) -> Verdict {
        let Some(mut c) = self.conn.take() else {
            let e = self.err.unwrap_or_else(|| "clamd: not connected".into());
            return unavailable(e);
        };
        let reply = async {
            c.write_all(&[0, 0, 0, 0]).await?;
            let mut buf = Vec::new();
            c.read_to_end(&mut buf).await?;
            Ok::<_, std::io::Error>(String::from_utf8_lossy(&buf).trim_end_matches('\0').trim().to_string())
        };
        match tokio::time::timeout(std::time::Duration::from_secs(120), reply).await {
            Ok(Ok(r)) if r.ends_with("OK") => Verdict::Clean,
            Ok(Ok(r)) if r.ends_with("FOUND") => {
                let sig = r.trim_start_matches("stream:").trim().trim_end_matches("FOUND").trim().to_string();
                Verdict::Infected(sig)
            }
            Ok(Ok(r)) => unavailable(format!("clamd: {r}")),
            Ok(Err(e)) => unavailable(format!("clamd: {e}")),
            Err(_) => unavailable("clamd: timed out".into()),
        }
    }
}

fn unavailable(e: String) -> Verdict {
    if clam_required() { Verdict::Unavailable(e) } else {
        tracing::warn!("{e} — upload not scanned (CLAMAV_REQUIRED is off)");
        Verdict::Skipped
    }
}

/// Lowercase extension without the dot.
pub fn ext(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

fn starts(h: &[u8], sig: &[u8]) -> bool { h.len() >= sig.len() && &h[..sig.len()] == sig }

const ZIP: &[&[u8]] = &[b"PK\x03\x04", b"PK\x05\x06", b"PK\x07\x08"];
const OLE: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

fn is_zip(h: &[u8]) -> bool { ZIP.iter().any(|s| starts(h, s)) }

/// Programs: refused unless the basket allows the extension by name.
fn is_program(h: &[u8]) -> bool {
    starts(h, b"MZ")                                   // Windows PE / DOS
        || starts(h, b"\x7FELF")                       // Linux
        || starts(h, b"\xCF\xFA\xED\xFE") || starts(h, b"\xCE\xFA\xED\xFE") // Mach-O
        || starts(h, b"\xFE\xED\xFA\xCF") || starts(h, b"\xFE\xED\xFA\xCE")
        || starts(h, b"\xCA\xFE\xBA\xBE")              // Mach-O universal / Java class
}

/// Does the head of the file match what its extension says it is?
/// Unknown extensions pass (we only check formats we know).
fn matches_ext(e: &str, h: &[u8]) -> bool {
    if h.is_empty() { return true; } // empty file: nothing to check
    match e {
        "pdf" => h.len() >= 5 && h[..h.len().min(1024)].windows(5).any(|w| w == b"%PDF-"),
        "docx" | "xlsx" | "pptx" | "odt" | "ods" | "odp" | "zip" | "jar" | "epub" | "xlsm" | "docm" | "pptm" => is_zip(h),
        "doc" | "xls" | "ppt" | "msg" => starts(h, OLE),
        "png" => starts(h, b"\x89PNG\r\n\x1a\n"),
        "jpg" | "jpeg" => starts(h, b"\xFF\xD8\xFF"),
        "gif" => starts(h, b"GIF87a") || starts(h, b"GIF89a"),
        "webp" => h.len() >= 12 && &h[..4] == b"RIFF" && &h[8..12] == b"WEBP",
        "tif" | "tiff" => starts(h, b"II*\0") || starts(h, b"MM\0*"),
        "bmp" => starts(h, b"BM"),
        "rtf" => starts(h, b"{\\rtf"),
        "7z" => starts(h, b"7z\xBC\xAF\x27\x1C"),
        "rar" => starts(h, b"Rar!\x1A\x07"),
        "gz" | "tgz" => starts(h, b"\x1F\x8B"),
        "mp4" | "mov" | "m4a" => h.len() >= 8 && &h[4..8] == b"ftyp",
        "mp3" => starts(h, b"ID3") || starts(h, b"\xFF\xFB") || starts(h, b"\xFF\xF3") || starts(h, b"\xFF\xF2"),
        "wav" => h.len() >= 12 && &h[..4] == b"RIFF" && &h[8..12] == b"WAVE",
        // Text formats must not be a binary program in disguise.
        "txt" | "csv" | "json" | "xml" | "html" | "htm" | "md" => !is_program(h),
        _ => true,
    }
}

/// Type check on the first bytes. `allowed` is the basket's allow-list of
/// extensions (empty = no list).
pub fn check_type(name: &str, head: &[u8], allowed: &[String]) -> Result<(), String> {
    let e = ext(name);
    if is_program(head) && !allowed.iter().any(|a| a.eq_ignore_ascii_case(&e)) {
        return Err(format!("ไฟล์ \"{name}\" เป็นโปรแกรม ซึ่งอัปโหลดไม่ได้"));
    }
    if !matches_ext(&e, head) {
        return Err(format!("เนื้อไฟล์ \"{name}\" ไม่ตรงกับนามสกุล .{e}"));
    }
    Ok(())
}

/// Turn a verdict into the refusal shown to the person, if any.
pub fn refusal(name: &str, v: &Verdict) -> Option<String> {
    match v {
        Verdict::Infected(sig) => {
            tracing::warn!(file = %name, signature = %sig, "upload refused: malware found");
            Some(format!("ไฟล์ \"{name}\" ไม่ผ่านการตรวจไวรัส ({sig})"))
        }
        Verdict::Unavailable(e) => {
            tracing::error!(file = %name, "upload refused: {e}");
            Some("ตรวจไวรัสไม่ได้ในขณะนี้ ลองอีกครั้งภายหลัง".into())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn types() {
        assert!(check_type("a.pdf", b"%PDF-1.7 ...", &[]).is_ok());
        assert!(check_type("a.pdf", b"PK\x03\x04", &[]).is_err());
        assert!(check_type("a.docx", b"PK\x03\x04....", &[]).is_ok());
        assert!(check_type("a.docx", b"%PDF-1.4", &[]).is_err());
        assert!(check_type("a.xls", OLE, &[]).is_ok());
        assert!(check_type("a.txt", b"MZ\x90\0", &[]).is_err());
        assert!(check_type("setup.exe", b"MZ\x90\0", &[]).is_err());
        assert!(check_type("setup.exe", b"MZ\x90\0", &["exe".into()]).is_ok());
        assert!(check_type("notes.unknown", b"\x00\x01", &[]).is_ok());
        assert!(check_type("empty.pdf", b"", &[]).is_ok());
    }
}
