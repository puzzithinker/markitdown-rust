use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

pub(crate) struct PdfConverter;

impl Converter for PdfConverter {
    fn name(&self) -> &'static str {
        "pdf"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(info, &[".pdf"]) || has_mime(info, &["application/pdf"])
    }

    fn convert(
        &self,
        bytes: &[u8],
        _info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let doc = if let Some(path) = ctx.path {
            unpdf::parse_file(path).map_err(|err| Error::Message(err.to_string()))?
        } else {
            unpdf::parse_bytes(bytes).map_err(|err| Error::Message(err.to_string()))?
        };
        let markdown = unpdf::render::to_markdown(&doc, &unpdf::render::RenderOptions::default())
            .map_err(|err| Error::Message(err.to_string()))?;
        let mut result = DocumentConverterResult::new(markdown);
        result.title = doc.metadata.title.filter(|title| !title.trim().is_empty());
        Ok(result)
    }
}
