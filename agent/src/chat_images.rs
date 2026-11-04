//! Clipboard pixels become pending PNG attachments. Only submitted images enter source assets.
use crate::{
    config::Protocol,
    workspace::{Workspace, hash},
};
use anyhow::{Context, Result, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use image::ImageEncoder;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{io::Read, sync::Arc};

pub const MAX_IMAGES: usize = 4;
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
const MAX_REQUEST_BYTES: usize = 20 * 1024 * 1024;
const MAX_PIXELS: usize = 32_000_000;

#[derive(Clone)]
pub struct Image {
    pub id: String,
    pub width: u32,
    pub height: u32,
    png: Arc<[u8]>,
}
impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Image")
            .field("id", &self.id)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("png_bytes", &self.png.len())
            .finish()
    }
}
impl Image {
    pub fn from_rgba(width: usize, height: usize, rgba: &[u8]) -> Result<Self> {
        let pixels = width
            .checked_mul(height)
            .context("Invalid image dimensions")?;
        ensure!(
            width > 0 && height > 0 && pixels <= MAX_PIXELS,
            "Image exceeds 32 megapixels; copy a smaller region"
        );
        ensure!(
            pixels.checked_mul(4) == Some(rgba.len()),
            "Invalid RGBA clipboard data"
        );
        let mut png = vec![];
        image::codecs::png::PngEncoder::new(&mut png).write_image(
            rgba,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )?;
        ensure!(
            png.len() <= MAX_IMAGE_BYTES,
            "PNG attachment exceeds 8 MiB; copy a smaller region"
        );
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            width: width as u32,
            height: height as u32,
            png: png.into(),
        })
    }
    pub fn bytes(&self) -> usize {
        self.png.len()
    }
    pub fn label(&self) -> String {
        format!("{}×{} PNG", self.width, self.height)
    }
}

pub enum ClipboardContent {
    Image(Image),
    Text(String),
}
/// Called only by an explicit paste action on a background worker; never polls or writes the clipboard.
pub fn clipboard() -> Result<ClipboardContent> {
    let mut clipboard = arboard::Clipboard::new()
        .context("System clipboard unavailable; a local graphical desktop is required")?;
    match clipboard.get_image() {
        Ok(image) => Ok(ClipboardContent::Image(Image::from_rgba(
            image.width,
            image.height,
            &image.bytes,
        )?)),
        Err(image_error) => {
            let text = clipboard.get_text().with_context(|| {
                format!("Clipboard has no readable image or text ({image_error})")
            })?;
            ensure!(
                text.len() <= 2 * 1024 * 1024,
                "Clipboard text exceeds 2 MiB"
            );
            Ok(ClipboardContent::Text(text))
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedImage {
    pub path: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    pub bytes: usize,
    pub source_note: String,
}
/// Uses ordinary source intake, preserving its language and pending-review contract.
pub fn save(workspace: &Workspace, images: &[Image]) -> Result<Vec<SavedImage>> {
    ensure!(
        images.len() <= MAX_IMAGES,
        "At most four images may be attached to one message"
    );
    ensure!(
        images.iter().map(Image::bytes).sum::<usize>() <= MAX_REQUEST_BYTES,
        "Attached PNG data exceeds 20 MiB; use fewer images"
    );
    let mut saved = vec![];
    for image in images {
        let digest = hash(&image.png);
        let directory = tempfile::tempdir()?;
        let input = directory.path().join("clipboard-image.png");
        std::fs::write(&input, &image.png)?;
        crate::sources::ingest(
            workspace,
            input
                .to_str()
                .context("Clipboard staging path is not UTF-8")?,
        )?;
        let source_note = format!("01_Sources/Notes/SRC-{}.md", &digest[..16]);
        let source = crate::records::read(workspace, &source_note)?;
        let path = source.metadata["asset"]
            .as_str()
            .context("Imported image has no asset")?
            .into();
        saved.push(SavedImage {
            path,
            sha256: digest,
            width: image.width,
            height: image.height,
            bytes: image.bytes(),
            source_note,
        });
    }
    Ok(saved)
}
pub fn source_descriptors(images: &[SavedImage]) -> String {
    images.iter().enumerate().map(|(i, image)| format!(
        "\n[Application image attachment {}: {}; SHA256: {}; {}x{} PNG; source note: {}; attachment does not establish source review]",
        i + 1, image.path, image.sha256, image.width, image.height, image.source_note
    )).collect()
}
pub fn message(text: String, images: &[SavedImage]) -> Value {
    if images.is_empty() {
        return json!({"role":"user","content":text});
    }
    let mut content = vec![json!({"type":"text","text":text})];
    content.extend(
        images
            .iter()
            .map(|image| json!({"type":"pm_image","image":image})),
    );
    json!({"role":"user","content":content})
}
/// Expand local image references for the selected wire protocol only. Never mutate stored history.
pub fn hydrate(
    workspace: &Workspace,
    history: &[Value],
    protocol: &Protocol,
) -> Result<Vec<Value>> {
    let mut wire = history.to_vec();
    let mut total = 0usize;
    for item in &mut wire {
        if item["role"] != "user" {
            continue;
        }
        let Some(parts) = item["content"].as_array_mut() else {
            continue;
        };
        if !parts.iter().any(|p| p["type"] == "pm_image") {
            continue;
        }
        for part in parts {
            if part["type"] == "text" && *protocol == Protocol::Responses {
                part["type"] = json!("input_text");
            }
            if part["type"] != "pm_image" {
                continue;
            }
            let saved: SavedImage = serde_json::from_value(part["image"].clone())
                .context("Invalid saved image reference")?;
            ensure!(
                saved.path.starts_with("01_Sources/Assets/") && saved.path.ends_with(".png"),
                "Image is outside the source asset scope"
            );
            ensure!(saved.bytes <= MAX_IMAGE_BYTES, "Saved image exceeds 8 MiB");
            total = total
                .checked_add(saved.bytes)
                .context("Image size overflow")?;
            ensure!(
                total <= MAX_REQUEST_BYTES,
                "Session image payload exceeds 20 MiB; start a new scoped session. No images were silently dropped"
            );
            let file = std::fs::File::open(workspace.path(&saved.path)?)
                .with_context(|| format!("Image unavailable: {}", saved.path))?;
            ensure!(
                file.metadata()?.is_file() && file.metadata()?.len() == saved.bytes as u64,
                "Image size changed: {}",
                saved.path
            );
            let mut png = Vec::with_capacity(saved.bytes);
            file.take(MAX_IMAGE_BYTES as u64 + 1)
                .read_to_end(&mut png)?;
            ensure!(
                png.len() == saved.bytes && hash(&png) == saved.sha256,
                "Image changed: {}; restore the original before continuing",
                saved.path
            );
            ensure!(
                png.len() >= 24 && &png[..8] == b"\x89PNG\r\n\x1a\n",
                "Saved attachment is not PNG"
            );
            let width = u32::from_be_bytes(png[16..20].try_into()?);
            let height = u32::from_be_bytes(png[20..24].try_into()?);
            ensure!(
                width == saved.width
                    && height == saved.height
                    && width > 0
                    && height > 0
                    && (width as u64 * height as u64) <= MAX_PIXELS as u64,
                "Saved image dimensions do not match"
            );
            let url = format!("data:image/png;base64,{}", STANDARD.encode(png));
            *part = match protocol {
                Protocol::Responses => {
                    json!({"type":"input_image","image_url":url,"detail":"auto"})
                }
                Protocol::ChatCompletions => {
                    json!({"type":"image_url","image_url":{"url":url,"detail":"auto"}})
                }
            };
        }
    }
    Ok(wire)
}

pub fn labels(item: &Value) -> Vec<String> {
    item["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| p["type"] == "pm_image")
        .enumerate()
        .map(|(i, p)| {
            format!(
                "[Image {} · {}×{} PNG]",
                i + 1,
                p["image"]["width"],
                p["image"]["height"]
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_pixels_and_never_places_payload_in_debug_output() {
        assert!(Image::from_rgba(0, 3, &[]).is_err());
        assert!(Image::from_rgba(usize::MAX, 2, &[]).is_err());
        assert!(Image::from_rgba(2, 2, &[0; 3]).is_err());
        let image = Image::from_rgba(2, 1, &[255; 8]).unwrap();
        let decoded = image::load_from_memory(&image.png).unwrap().to_rgba8();
        assert_eq!(decoded.as_raw(), &[255; 8]);
        assert!(!format!("{image:?}").contains("255, 255"));
    }
}
