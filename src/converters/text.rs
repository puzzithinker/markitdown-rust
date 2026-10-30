use crate::charset::decode_text;
use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, mime_starts_with};
use crate::error::Result;
use crate::stream_info::StreamInfo;

pub(crate) struct PlainTextConverter;

impl Converter for PlainTextConverter {
    fn name(&self) -> &'static str {
        "plain-text"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(
            info,
            &[
                ".txt",
                ".text",
                ".md",
                ".markdown",
                ".json",
                ".jsonl",
                ".xml",
            ],
        ) || mime_starts_with(
            info,
            &[
                "text/",
                "application/json",
                "application/markdown",
                "application/xml",
                "text/xml",
            ],
        ) || info.charset.is_some()
    }

    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        _ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        Ok(DocumentConverterResult::new(decode_text(
            bytes,
            info.charset.as_deref(),
        )))
    }
}
