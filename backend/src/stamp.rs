//! Download stamp: every page of a downloaded PDF gets a footer line and a faint
//! diagonal watermark naming who downloaded it, when, the file id and a copy
//! code. The copy code is logged in the activity feed, so a leaked printout can
//! be traced back to one download.
//!
//! Text uses the PDF base-14 Helvetica font (no embedding), so it is ASCII:
//! the downloader's email and ids. Thai names stay in the activity log.

use lopdf::{content::{Content, Operation}, dictionary, Document, Object, ObjectId, Stream};

pub struct StampInfo<'a> {
    pub email: &'a str,
    pub when: &'a str,
    pub file_id: &'a str,
    pub copy_code: &'a str,
    pub org: &'a str,
}

fn ascii(s: &str) -> String {
    s.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '?' }).collect::<String>()
        .replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)")
}

/// Returns the stamped PDF, or None when the bytes aren't a PDF lopdf can edit.
pub fn stamp_pdf(pdf: &[u8], info: &StampInfo) -> Option<Vec<u8>> {
    let mut doc = Document::load_mem(pdf).ok()?;
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
    });
    let gs_id = doc.add_object(dictionary! { "Type" => "ExtGState", "ca" => 0.12, "CA" => 0.12 });
    let footer = ascii(&format!(
        "Downloaded by {} {}on {} | File {} | Copy {}",
        info.email, if info.org.is_empty() { String::new() } else { format!("({}) ", info.org) }, info.when, info.file_id, info.copy_code,
    ));
    let mark = ascii(&format!("{}  {}", info.email, info.copy_code));
    let pages: Vec<ObjectId> = doc.get_pages().values().cloned().collect();
    for pid in pages {
        let (w, h) = page_size(&doc, pid).unwrap_or((595.0, 842.0));
        let ops = vec![
            Operation::new("q", vec![]),
            // footer
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["FHStamp".into(), 7.into()]),
            Operation::new("rg", vec![0.35.into(), 0.35.into(), 0.35.into()]),
            Operation::new("Td", vec![24.into(), 12.into()]),
            Operation::new("Tj", vec![Object::string_literal(footer.clone())]),
            Operation::new("ET", vec![]),
            // diagonal watermark
            Operation::new("gs", vec!["FHAlpha".into()]),
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["FHStamp".into(), 28.into()]),
            Operation::new("rg", vec![0.5.into(), 0.5.into(), 0.5.into()]),
            Operation::new("Tm", vec![0.7071.into(), 0.7071.into(), (-0.7071).into(), 0.7071.into(), (w * 0.18).into(), (h * 0.3).into()]),
            Operation::new("Tj", vec![Object::string_literal(mark.clone())]),
            Operation::new("ET", vec![]),
            Operation::new("Q", vec![]),
        ];
        let content = Content { operations: ops }.encode().ok()?;
        let sid = doc.add_object(Stream::new(dictionary! {}, content));
        // Wrap the original content in q/Q so its graphics state can't leak into the stamp.
        let pre = doc.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
        let post = doc.add_object(Stream::new(dictionary! {}, b"\nQ\n".to_vec()));
        {
            let page = doc.get_object_mut(pid).ok()?.as_dict_mut().ok()?;
            let mut arr: Vec<Object> = vec![Object::Reference(pre)];
            match page.get(b"Contents") {
                Ok(Object::Reference(r)) => arr.push(Object::Reference(*r)),
                Ok(Object::Array(a)) => arr.extend(a.iter().cloned()),
                _ => {}
            }
            arr.push(Object::Reference(post));
            arr.push(Object::Reference(sid));
            page.set("Contents", Object::Array(arr));
        }
        add_resources(&mut doc, pid, font_id, gs_id)?;
    }
    doc.compress();
    let mut out = Vec::new();
    doc.save_to(&mut out).ok()?;
    Some(out)
}

fn page_size(doc: &Document, pid: ObjectId) -> Option<(f64, f64)> {
    let mut cur = doc.get_object(pid).ok()?.as_dict().ok()?.clone();
    for _ in 0..16 {
        if let Ok(mb) = cur.get(b"MediaBox") {
            let arr = match mb { Object::Array(a) => a.clone(), Object::Reference(r) => doc.get_object(*r).ok()?.as_array().ok()?.clone(), _ => return None };
            let n = |o: &Object| o.as_float().map(|f| f as f64).or_else(|_| o.as_i64().map(|i| i as f64)).ok();
            return Some((n(&arr[2])? - n(&arr[0])?, n(&arr[3])? - n(&arr[1])?));
        }
        let parent = cur.get(b"Parent").ok()?.as_reference().ok()?;
        cur = doc.get_object(parent).ok()?.as_dict().ok()?.clone();
    }
    None
}

/// Give the page its own Resources (copied from inherited/indirect ones) and add
/// the stamp font + transparency state.
fn add_resources(doc: &mut Document, pid: ObjectId, font_id: ObjectId, gs_id: ObjectId) -> Option<()> {
    // Resolve the effective resources dictionary (own, indirect, or inherited).
    let mut res = lopdf::Dictionary::new();
    let mut cur = Some(pid);
    for _ in 0..16 {
        let Some(id) = cur else { break };
        let d = doc.get_object(id).ok()?.as_dict().ok()?.clone();
        if let Ok(r) = d.get(b"Resources") {
            res = match r { Object::Dictionary(x) => x.clone(), Object::Reference(rr) => doc.get_object(*rr).ok()?.as_dict().ok()?.clone(), _ => lopdf::Dictionary::new() };
            break;
        }
        cur = d.get(b"Parent").ok().and_then(|p| p.as_reference().ok());
    }
    let mut fonts = match res.get(b"Font") {
        Ok(Object::Dictionary(x)) => x.clone(),
        Ok(Object::Reference(r)) => doc.get_object(*r).ok()?.as_dict().ok()?.clone(),
        _ => lopdf::Dictionary::new(),
    };
    fonts.set("FHStamp", Object::Reference(font_id));
    let mut gs = match res.get(b"ExtGState") {
        Ok(Object::Dictionary(x)) => x.clone(),
        Ok(Object::Reference(r)) => doc.get_object(*r).ok()?.as_dict().ok()?.clone(),
        _ => lopdf::Dictionary::new(),
    };
    gs.set("FHAlpha", Object::Reference(gs_id));
    res.set("Font", Object::Dictionary(fonts));
    res.set("ExtGState", Object::Dictionary(gs));
    doc.get_object_mut(pid).ok()?.as_dict_mut().ok()?.set("Resources", Object::Dictionary(res));
    Some(())
}

pub fn copy_code() -> String {
    const A: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut r = rand::thread_rng();
    (0..8).map(|_| A[rand::Rng::gen_range(&mut r, 0..A.len())] as char).collect()
}
