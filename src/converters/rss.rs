use std::io::Cursor;

use crate::charset::decode_text;
use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::html::convert_html_str;
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

pub(crate) struct RssConverter;

impl Converter for RssConverter {
    fn name(&self) -> &'static str {
        "rss"
    }

    fn accepts(&self, info: &StreamInfo, bytes: &[u8]) -> bool {
        if has_extension(info, &[".rss", ".atom"])
            || has_mime(
                info,
                &[
                    "application/rss+xml",
                    "application/rss",
                    "application/atom+xml",
                    "application/atom",
                ],
            )
        {
            return true;
        }
        let xmlish = has_extension(info, &[".xml"])
            || has_mime(
                info,
                &["application/xml", "text/xml", "application/rss+xml"],
            );
        xmlish && looks_like_feed(bytes)
    }

    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let text = decode_text(bytes, info.charset.as_deref());
        let feed = feed_rs::parser::parse(Cursor::new(text.as_bytes()))
            .map_err(|err| Error::Message(err.to_string()))?;
        let title = feed
            .title
            .as_ref()
            .map(|text| text.content.trim().to_string());
        let mut markdown = String::new();
        if let Some(title) = title.as_ref().filter(|title| !title.is_empty()) {
            markdown.push_str(&format!("# {title}\n"));
        }
        if let Some(description) = feed.description.as_ref() {
            let description = description.content.trim();
            if !description.is_empty() {
                markdown.push_str(description);
                markdown.push('\n');
            }
        }
        for entry in &feed.entries {
            if let Some(entry_title) = entry.title.as_ref().map(|text| text.content.trim()) {
                if !entry_title.is_empty() {
                    markdown.push_str(&format!("\n## {entry_title}\n"));
                }
            }
            if let Some(published) = entry.published.as_ref() {
                markdown.push_str(&format!("Published on: {published}\n"));
            }
            if let Some(updated) = entry.updated.as_ref() {
                markdown.push_str(&format!("Updated on: {updated}\n"));
            }
            if let Some(link) = entry.links.first() {
                if !link.href.is_empty() {
                    markdown.push_str(&format!("{}\n", link.href));
                }
            }
            let body = entry_body(entry, ctx.options.keep_data_uris)?;
            if !body.is_empty() {
                if !markdown.ends_with('\n') {
                    markdown.push_str("\n\n");
                } else if !markdown.ends_with("\n\n") {
                    markdown.push('\n');
                }
                markdown.push_str(&body);
            }
        }
        let mut result = DocumentConverterResult::new(markdown);
        result.title = title.filter(|title| !title.is_empty());
        Ok(result)
    }
}

fn looks_like_feed(bytes: &[u8]) -> bool {
    let sample = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let lower = sample.to_ascii_lowercase();
    let trimmed = lower.trim_start();
    trimmed.starts_with("<rss")
        || trimmed.starts_with("<feed")
        || (trimmed.contains("<rss") && trimmed.contains("<channel"))
        || (trimmed.contains("<feed") && trimmed.contains("<entry"))
}

fn entry_body(entry: &feed_rs::model::Entry, keep_data_uris: bool) -> Result<String> {
    let mut parts = Vec::new();
    if let Some(content) = entry.content.as_ref() {
        if let Some(body) = content.body.as_deref() {
            parts.push(render_feed_text(
                body,
                content.content_type.as_ref(),
                keep_data_uris,
            )?);
        }
    }
    if parts.iter().all(|part| part.trim().is_empty()) {
        if let Some(summary) = entry.summary.as_ref() {
            parts.clear();
            parts.push(render_feed_text(
                &summary.content,
                summary.content_type.as_ref(),
                keep_data_uris,
            )?);
        }
    }
    Ok(parts
        .into_iter()
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n"))
}

fn render_feed_text(value: &str, content_type: &str, keep_data_uris: bool) -> Result<String> {
    let kind = content_type.to_ascii_lowercase();
    let markup = kind.contains("html") || kind.contains("xml") || value.contains('<');
    if markup {
        Ok(convert_html_str(value, keep_data_uris)?.markdown)
    } else {
        Ok(value.trim().to_string())
    }
}
