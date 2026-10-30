use std::io::Cursor;

use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

const EXTENSIONS: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".gif", ".webp", ".bmp", ".tif", ".tiff",
];

const MIMES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/bmp",
    "image/tiff",
    "image/x-ms-bmp",
];

pub(crate) struct ImageConverter;

impl Converter for ImageConverter {
    fn name(&self) -> &'static str {
        "image"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(info, EXTENSIONS) || has_mime(info, MIMES)
    }

    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        _ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let mut lines = Vec::new();
        if let Some(format) = image_format(bytes, info) {
            lines.push(format!("- Format: {format}"));
        }
        if let Ok(size) = imagesize::blob_size(bytes) {
            lines.push(format!("- Dimensions: {} x {}", size.width, size.height));
        }
        append_exif(bytes, &mut lines);
        if lines.is_empty() {
            return Err(Error::Message("could not read image metadata".into()));
        }
        Ok(DocumentConverterResult::new(lines.join("\n")))
    }
}

fn image_format(bytes: &[u8], info: &StreamInfo) -> Option<String> {
    if let Ok(kind) = imagesize::image_type(bytes) {
        return Some(match kind {
            imagesize::ImageType::Jpeg => "JPEG".to_string(),
            imagesize::ImageType::Png => "PNG".to_string(),
            imagesize::ImageType::Gif => "GIF".to_string(),
            imagesize::ImageType::Webp => "WebP".to_string(),
            imagesize::ImageType::Bmp => "BMP".to_string(),
            imagesize::ImageType::Tiff => "TIFF".to_string(),
            other => format!("{other:?}"),
        });
    }
    info.extension
        .as_deref()
        .map(|extension| extension.trim_start_matches('.').to_ascii_uppercase())
}

fn append_exif(bytes: &[u8], lines: &mut Vec<String>) {
    let mut cursor = Cursor::new(bytes);
    let Ok(exif) = exif::Reader::new().read_from_container(&mut cursor) else {
        return;
    };
    for field in exif.fields().take(40) {
        let value = field.display_value().with_unit(&exif).to_string();
        let value = value.trim();
        if value.is_empty() || value.len() > 300 {
            continue;
        }
        lines.push(format!("- {}: {value}", field.tag));
    }
}
