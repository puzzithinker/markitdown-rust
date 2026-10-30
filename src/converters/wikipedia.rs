use crate::charset::decode_text;
use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::html::{
    convert_html_str, is_wikipedia_url, looks_like_html, wikipedia_fragment,
};
use crate::error::Result;
use crate::stream_info::StreamInfo;

pub(crate) struct WikipediaConverter;

impl Converter for WikipediaConverter {
    fn name(&self) -> &'static str {
        "wikipedia"
    }

    fn accepts(&self, info: &StreamInfo, bytes: &[u8]) -> bool {
        is_wikipedia_url(info.url.as_deref()) && looks_like_html(info, bytes)
    }

    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let html = decode_text(bytes, info.charset.as_deref());
        let fragment = wikipedia_fragment(&html).unwrap_or(html);
        convert_html_str(&fragment, ctx.options.keep_data_uris)
    }
}
