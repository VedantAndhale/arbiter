//! Attachments, stored once and shaped for cheap model consumption:
//! - content-addressed (same file twice costs nothing),
//! - images downscaled to a 1568px long edge and recompressed,
//! - PDFs converted to text,
//! - never inlined into prompts: agents get a path plus a one-line note and
//!   read only what they need with their own tools.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_EDGE: u32 = 1568;
const LARGE_TEXT: usize = 32 * 1024;
pub const MAX_UPLOAD: usize = 50 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Attachment {
    /// Content hash prefix; also the stored file stem.
    pub id: String,
    /// Original file name, for display.
    pub name: String,
    /// `image` | `pdf` | `text` | `file`
    pub kind: String,
    /// Stored file name (`<id>.<ext>`), possibly converted (e.g. PDF → .txt).
    pub file: String,
    /// One line the agent sees, e.g. "image 1568×882 (downscaled from 3840×2160)".
    pub note: String,
    pub bytes: u64,
}

fn store_dir(home: &Path) -> PathBuf {
    home.join("attachments")
}

fn ext_of(name: &str) -> String {
    Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Process and store an upload in the daemon's attachment store.
pub fn ingest(home: &Path, name: &str, data: &[u8]) -> Result<Attachment> {
    anyhow::ensure!(data.len() <= MAX_UPLOAD, "file exceeds 50 MB upload limit");
    if data.is_empty() {
        bail!("empty file");
    }
    let id = hex(&Sha256::digest(data))[..16].to_owned();
    let dir = store_dir(home);
    std::fs::create_dir_all(&dir)?;
    let meta_path = dir.join(format!("{id}.json"));
    let safe_name: String = name.chars().filter(|c| !c.is_control() && !"/\\:*?\"<>|".contains(*c)).take(120).collect();
    if let Ok(m) = std::fs::read(&meta_path)
        && let Ok(a) = serde_json::from_slice::<Attachment>(&m)
    {
        return Ok(Attachment { name: safe_name, ..a }); // dedupe
    }
    let ext = ext_of(name);
    let (kind, file, note, bytes) = match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => image(&dir, &id, data)?,
        "pdf" => pdf(&dir, &id, data)?,
        "docx" | "xlsx" | "pptx" => {
            let text = crate::office::extract(data, &ext)?;
            let file = format!("{id}.txt");
            std::fs::write(dir.join(&file), &text)?;
            (
                "document".into(),
                file,
                format!("{} converted to text, {} KB", ext.to_uppercase(), text.len() / 1024),
                text.len() as u64,
            )
        }
        _ if looks_textual(data) => {
            let file = format!("{id}.{}", if ext.is_empty() { "txt".into() } else { ext.clone() });
            std::fs::write(dir.join(&file), data)?;
            let lines = data.iter().filter(|b| **b == b'\n').count() + 1;
            let note = if data.len() > LARGE_TEXT {
                format!("large text file, {lines} lines ({} KB); read only the parts you need", data.len() / 1024)
            } else {
                format!("text, {lines} lines")
            };
            ("text".to_owned(), file, note, data.len() as u64)
        }
        _ => {
            let file = format!("{id}.{}", if ext.is_empty() { "bin".into() } else { ext.clone() });
            std::fs::write(dir.join(&file), data)?;
            ("file".to_owned(), file, format!("binary file ({} KB)", data.len() / 1024), data.len() as u64)
        }
    };
    let a = Attachment { id, name: safe_name, kind, file, note, bytes };
    std::fs::write(&meta_path, serde_json::to_vec(&a)?)?;
    Ok(a)
}

pub fn get(home: &Path, id: &str) -> Result<(Attachment, PathBuf)> {
    if id.len() != 16 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid attachment id");
    }
    let dir = store_dir(home);
    let a: Attachment =
        serde_json::from_slice(&std::fs::read(dir.join(format!("{id}.json"))).context("unknown attachment")?)?;
    let path = dir.join(&a.file);
    Ok((a, path))
}

/// The prompt lines for attachments, by absolute path in Arbiter's own
/// store. Nothing is copied into the project; agents get read access to the
/// store (see `attachment_dir`).
pub async fn materialize(home: &Path, _cwd: &Path, ids: &[String]) -> Result<(Vec<Attachment>, String)> {
    anyhow::ensure!(ids.len() <= 10, "at most 10 attachments per message");
    if ids.is_empty() {
        return Ok((Vec::new(), String::new()));
    }
    let mut out = Vec::new();
    let mut lines = String::from("\n\nAttached files (read them with your file tools only if needed):");
    let mut seen = std::collections::HashSet::new();
    for id in ids {
        if !seen.insert(id) {
            continue;
        }
        let (a, src) = get(home, id)?;
        lines.push_str(&format!("\n- {} — {}: {}", src.display(), a.name, a.note));
        out.push(a);
    }
    Ok((out, lines))
}

/// Where attachments are stored; agents may read it.
pub fn attachment_dir(home: &Path) -> PathBuf {
    store_dir(home)
}

fn image(dir: &Path, id: &str, data: &[u8]) -> Result<(String, String, String, u64)> {
    let img = image::load_from_memory(data).context("unreadable image")?;
    let (w, h) = (img.width(), img.height());
    let resized =
        if w.max(h) > MAX_EDGE { img.resize(MAX_EDGE, MAX_EDGE, image::imageops::FilterType::Triangle) } else { img };
    let (nw, nh) = (resized.width(), resized.height());
    // Model token cost follows pixel dimensions (fixed above); for bytes, keep
    // whichever encoding is smaller: PNG wins on flat UI screenshots, JPEG on photos.
    let mut png = std::io::Cursor::new(Vec::new());
    resized.write_to(&mut png, image::ImageFormat::Png)?;
    let png = png.into_inner();
    let (bytes, ext) = if resized.color().has_alpha() {
        (png, "png")
    } else {
        let mut jpg = std::io::Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpg, 82).encode_image(&resized.to_rgb8())?;
        let jpg = jpg.into_inner();
        if jpg.len() < png.len() { (jpg, "jpg") } else { (png, "png") }
    };
    // Keep the original if re-encoding didn't help (already small/optimized).
    let (bytes, ext) = if nw == w && bytes.len() >= data.len() { (data.to_vec(), "orig") } else { (bytes, ext) };
    let ext = if ext == "orig" { guess_image_ext(data) } else { ext };
    let file = format!("{id}.{ext}");
    std::fs::write(dir.join(&file), &bytes)?;
    let note = if (nw, nh) != (w, h) {
        format!("image {nw}×{nh} (downscaled from {w}×{h})")
    } else {
        format!("image {w}×{h}")
    };
    Ok(("image".into(), file, note, bytes.len() as u64))
}

fn guess_image_ext(data: &[u8]) -> &'static str {
    match data {
        [0x89, b'P', b'N', b'G', ..] => "png",
        [0xFF, 0xD8, ..] => "jpg",
        [b'G', b'I', b'F', ..] => "gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => "webp",
        _ => "img",
    }
}

fn pdf(dir: &Path, id: &str, data: &[u8]) -> Result<(String, String, String, u64)> {
    std::fs::write(dir.join(format!("{id}.pdf")), data)?;
    // pdf-extract can panic on odd files; contain it.
    let text = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(data)).ok().and_then(|r| r.ok());
    match text.filter(|t| !t.trim().is_empty()) {
        Some(t) => {
            let file = format!("{id}.txt");
            std::fs::write(dir.join(&file), &t)?;
            let pages = t.matches('\u{c}').count().max(1);
            Ok((
                "pdf".into(),
                file,
                format!("PDF converted to text (~{pages} pages, {} KB)", t.len() / 1024),
                t.len() as u64,
            ))
        }
        None => {
            Ok(("pdf".into(), format!("{id}.pdf"), "PDF (no extractable text; scanned?)".into(), data.len() as u64))
        }
    }
}

fn looks_textual(data: &[u8]) -> bool {
    let sample = &data[..data.len().min(8192)];
    !sample.contains(&0) && std::str::from_utf8(sample).map(|_| true).unwrap_or_else(|e| e.error_len().is_none())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downscales_large_images_and_dedupes() {
        let home = tempfile::tempdir().unwrap();
        let img = image::RgbImage::from_fn(3200, 1800, |x, y| image::Rgb([(x % 255) as u8, (y % 255) as u8, 128]));
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(img).write_to(&mut png, image::ImageFormat::Png).unwrap();
        let png = png.into_inner();
        let a = ingest(home.path(), "shot.png", &png).unwrap();
        assert_eq!(a.kind, "image");
        assert!(a.note.contains("1568×882") && a.note.contains("3200×1800"), "{}", a.note);
        assert!(a.bytes < png.len() as u64, "{} vs {}", a.bytes, png.len());
        let stored = image::open(home.path().join("attachments").join(&a.file)).unwrap();
        assert_eq!((stored.width(), stored.height()), (1568, 882));
        let again = ingest(home.path(), "copy.png", &png).unwrap();
        assert_eq!((again.id.as_str(), again.name.as_str()), (a.id.as_str(), "copy.png"));
    }

    #[test]
    fn text_notes_and_bad_ids() {
        let home = tempfile::tempdir().unwrap();
        let big = "line\n".repeat(10_000);
        let a = ingest(home.path(), "server.log", big.as_bytes()).unwrap();
        assert!(a.kind == "text" && a.note.contains("read only the parts you need"), "{a:?}");
        assert!(get(home.path(), "../etc").is_err());
    }

    #[tokio::test]
    async fn materialize_copies_and_excludes_from_git() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        std::process::Command::new("git").arg("-C").arg(repo.path()).args(["init", "-q"]).status().unwrap();
        let a = ingest(home.path(), "notes.md", b"# hi\n").unwrap();
        let (list, lines) = materialize(home.path(), repo.path(), std::slice::from_ref(&a.id)).await.unwrap();
        assert_eq!(list.len(), 1);
        assert!(lines.contains(&a.file) && lines.contains("notes.md"), "{lines}");
        // Nothing is written into the project.
        assert!(!repo.path().join(".arbiter").exists());
        let status = std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&status.stdout).trim().is_empty(), "attachments are git-excluded");
    }
}
