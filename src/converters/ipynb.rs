use serde_json::Value;

use crate::charset::decode_text;
use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

pub(crate) struct IpynbConverter;

impl Converter for IpynbConverter {
    fn name(&self) -> &'static str {
        "ipynb"
    }

    fn accepts(&self, info: &StreamInfo, bytes: &[u8]) -> bool {
        if has_extension(info, &[".ipynb"]) {
            return true;
        }
        if has_extension(info, &[".json"]) || has_mime(info, &["application/json"]) {
            let sample = String::from_utf8_lossy(&bytes[..bytes.len().min(8192)]);
            return sample.contains("nbformat") && sample.contains("cells");
        }
        false
    }

    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        _ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let text = decode_text(bytes, info.charset.as_deref());
        let notebook: Value = serde_json::from_str(&text)
            .map_err(|err| Error::Message(format!("Error converting .ipynb file: {err}")))?;
        let language = notebook
            .pointer("/metadata/language_info/name")
            .and_then(Value::as_str)
            .unwrap_or("python");
        let mut title = notebook
            .pointer("/metadata/title")
            .and_then(Value::as_str)
            .map(str::to_string);
        let mut blocks = Vec::new();
        let cells = notebook
            .get("cells")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Message("Error converting .ipynb file: missing cells".into()))?;
        for cell in cells {
            let cell_type = cell.get("cell_type").and_then(Value::as_str).unwrap_or("");
            let source = cell_source(cell.get("source"));
            match cell_type {
                "markdown" => {
                    if title.is_none() {
                        title = source.lines().find_map(|line| {
                            line.strip_prefix("# ")
                                .map(str::trim)
                                .filter(|title| !title.is_empty())
                                .map(str::to_string)
                        });
                    }
                    blocks.push(source);
                }
                "code" => blocks.push(format!("```{language}\n{source}\n```")),
                "raw" => blocks.push(format!("```\n{source}\n```")),
                _ => {}
            }
        }
        let mut result = DocumentConverterResult::new(blocks.join("\n\n"));
        result.title = title.filter(|title| !title.is_empty());
        Ok(result)
    }
}

fn cell_source(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(parts)) => parts.iter().filter_map(Value::as_str).collect::<String>(),
        _ => String::new(),
    }
}
