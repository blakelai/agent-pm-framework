//! Text extraction only: source instructions and visual layouts remain unreviewed data.
use anyhow::{Context, Result, ensure};
use std::{
    io::{Cursor, Read},
    path::Path,
};

pub(crate) const IMAGES: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"];
const EXPANDED_LIMIT: u64 = 200_000_000;
const TEXT_LIMIT: usize = 50_000_000;

pub(crate) fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}
pub(crate) struct Extraction {
    pub chunks: Vec<(String, String)>,
    pub status: &'static str,
}
pub(crate) fn extract(raw: &[u8], ext: &str) -> Result<Extraction> {
    let mut chunks = vec![];
    let status = match ext {
        "md" | "txt" | "csv" | "tsv" | "json" => {
            let text = std::str::from_utf8(raw)?
                .strip_prefix('\u{feff}')
                .unwrap_or(std::str::from_utf8(raw)?);
            chunks.push((
                "text".into(),
                text.replace("\r\n", "\n").replace('\r', "\n"),
            ));
            "text_extracted"
        }
        ext if IMAGES.contains(&ext) => "visual_review_required",
        "pdf" => {
            let doc = lopdf::Document::load_mem(raw).context("Cannot parse PDF")?;
            // Some PDFs can be opened with an empty password. They still need explicit review.
            if doc.is_encrypted() || doc.was_encrypted() {
                return Ok(Extraction {
                    chunks,
                    status: "encrypted_source",
                });
            }
            let pages = doc.get_pages();
            ensure!(
                pages.len() <= 10_000,
                "PDF exceeds 10,000 pages; provide a smaller source"
            );
            let mut size = 0;
            for page in pages.keys() {
                let text = doc.extract_text_with_limit(&[*page], 20_000_000)?;
                size += text.len();
                ensure!(
                    size <= TEXT_LIMIT,
                    "Extracted text exceeds 50 MB; provide a smaller source"
                );
                chunks.push((
                    format!("page-{page}"),
                    if text.trim().is_empty() {
                        String::new()
                    } else {
                        text
                    },
                ));
            }
            "visual_review_required"
        }
        "pptx" | "docx" => {
            let mut archive = zip::ZipArchive::new(Cursor::new(raw))?;
            ensure!(
                archive.len() <= 20_000,
                "Office package exceeds 20,000 entries"
            );
            let slide = regex::Regex::new(r"^ppt/slides/slide(\d+)\.xml$")?;
            let mut names = vec![];
            let mut size = 0u64;
            let mut unique = std::collections::BTreeSet::new();
            for i in 0..archive.len() {
                let file = archive.by_index(i)?;
                let name = file.name()?.into_owned();
                ensure!(unique.insert(name.clone()), "Duplicate Office ZIP entry");
                size = size
                    .checked_add(file.size())
                    .context("Office package size overflow")?;
                ensure!(
                    size <= EXPANDED_LIMIT,
                    "Expanded Office package exceeds 200 MB"
                );
                if ext == "pptx"
                    && let Some(c) = slide.captures(&name)
                {
                    names.push((c[1].parse::<u64>()?, name));
                }
            }
            names.sort();
            if ext == "docx" {
                names = vec![(1, "word/document.xml".into())];
            }
            let mut text_size = 0;
            for (i, (_, name)) in names.iter().enumerate() {
                let file = archive.by_name(name)?;
                let mut data = String::new();
                file.take(EXPANDED_LIMIT + 1).read_to_string(&mut data)?;
                ensure!(
                    data.len() as u64 <= EXPANDED_LIMIT,
                    "Office XML exceeds expansion limit"
                );
                // Default parser rejects DTDs, including external entities.
                let xml = roxmltree::Document::parse(&data)?;
                let text = xml
                    .descendants()
                    .filter(|n| n.is_element() && n.tag_name().name() == "t")
                    .map(|n| {
                        n.descendants()
                            .filter(|n| n.is_text())
                            .filter_map(|n| n.text())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                text_size += text.len();
                ensure!(
                    text_size <= TEXT_LIMIT,
                    "Extracted text exceeds 50 MB; provide a smaller source"
                );
                chunks.push((
                    if ext == "pptx" {
                        format!("slide-{}", i + 1)
                    } else {
                        "document-body".into()
                    },
                    text,
                ));
            }
            "visual_review_required"
        }
        _ => "unsupported_format",
    };
    Ok(Extraction { chunks, status })
}
