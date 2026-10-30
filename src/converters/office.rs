use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

const EXTENSIONS: &[&str] = &[".docx", ".xlsx", ".pptx", ".doc", ".xls", ".ppt"];

const MIMES: &[&str] = &[
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    "application/msword",
    "application/vnd.ms-excel",
    "application/vnd.ms-powerpoint",
];

pub(crate) struct OfficeConverter;

impl Converter for OfficeConverter {
    fn name(&self) -> &'static str {
        "office"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(info, EXTENSIONS) || has_mime(info, MIMES)
    }

    fn convert(
        &self,
        bytes: &[u8],
        _info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let doc = if let Some(path) = ctx.path {
            undoc::parse_file(path).map_err(|err| Error::Message(err.to_string()))?
        } else {
            undoc::parse_bytes(bytes).map_err(|err| Error::Message(err.to_string()))?
        };
        let markdown = undoc::render::to_markdown(&doc, &undoc::render::RenderOptions::default())
            .map_err(|err| Error::Message(err.to_string()))?;
        let mut result = DocumentConverterResult::new(markdown);
        result.title = doc.metadata.title.filter(|title| !title.trim().is_empty());
        Ok(result)
    }
}
