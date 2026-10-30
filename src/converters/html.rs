use scraper::{Html, Selector};

use crate::charset::decode_text;
use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, has_mime, mime_starts_with};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

pub(crate) struct HtmlConverter;

impl Converter for HtmlConverter {
    fn name(&self) -> &'static str {
        "html"
    }

    fn accepts(&self, info: &StreamInfo, bytes: &[u8]) -> bool {
        looks_like_html(info, bytes)
    }

    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let html = decode_text(bytes, info.charset.as_deref());
        convert_html_str(&html, ctx.options.keep_data_uris)
    }
}

pub(crate) fn looks_like_html(info: &StreamInfo, _bytes: &[u8]) -> bool {
    has_extension(info, &[".html", ".htm", ".xhtml"])
        || has_mime(info, &["text/html", "application/xhtml+xml"])
        || mime_starts_with(info, &["text/html", "application/xhtml"])
}

pub(crate) fn convert_html_str(
    html: &str,
    keep_data_uris: bool,
) -> Result<DocumentConverterResult> {
    let document = Html::parse_document(html);
    let title = element_text(&document, "title");
    let body_selector = Selector::parse("body").expect("body selector");
    let fragment = document
        .select(&body_selector)
        .next()
        .map(|body| body.inner_html())
        .unwrap_or_else(|| html.to_string());
    let converter = htmd::HtmlToMarkdown::builder()
        .skip_tags(vec!["script", "style", "noscript"])
        .build();
    let mut markdown = converter
        .convert(&fragment)
        .map_err(|err| Error::Message(err.to_string()))?;
    if !keep_data_uris {
        markdown = truncate_data_uris(&markdown);
    }
    let mut result = DocumentConverterResult::new(markdown.trim());
    result.title = title;
    Ok(result)
}

/// Keep the article body of a saved or fetched Wikipedia page.
pub(crate) fn wikipedia_fragment(html: &str) -> Option<String> {
    let mut document = Html::parse_document(html);
    let content = Selector::parse("#mw-content-text").ok()?;
    document.select(&content).next()?;
    let drop = Selector::parse(
        "#toc, .toc, .navbox, .reflist, .mw-editsection, .reference, .mw-references-wrap, .noprint",
    )
    .ok()?;
    let ids: Vec<_> = document.select(&drop).map(|element| element.id()).collect();
    for id in ids {
        if let Some(mut node) = document.tree.get_mut(id) {
            node.detach();
        }
    }
    document
        .select(&content)
        .next()
        .map(|element| element.inner_html())
}

pub(crate) fn is_wikipedia_url(url: Option<&str>) -> bool {
    let Some(url) = url else {
        return false;
    };
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "wikipedia.org" || host.ends_with(".wikipedia.org")
}

fn element_text(document: &Html, selector: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<String>()
        .trim()
        .to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn truncate_data_uris(markdown: &str) -> String {
    let bytes = markdown.as_bytes();
    let mut out = String::with_capacity(markdown.len());
    let mut index = 0;
    while index < bytes.len() {
        if markdown.is_char_boundary(index)
            && bytes[index..].starts_with(b"data:")
            && (index == 0 || !bytes[index - 1].is_ascii_alphanumeric())
        {
            let start = index;
            index += 5;
            while index < bytes.len()
                && !matches!(
                    bytes[index],
                    b' ' | b'\n' | b'\r' | b'\t' | b')' | b']' | b'"' | b'\'' | b'>'
                )
            {
                index += 1;
            }
            if index - start > 64 {
                out.push_str("data:...");
            } else {
                out.push_str(&markdown[start..index]);
            }
        } else {
            let ch = markdown[index..].chars().next().unwrap_or('\u{FFFD}');
            out.push(ch);
            index += ch.len_utf8();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{is_wikipedia_url, truncate_data_uris};

    #[test]
    fn wikipedia_host_matches_the_site_and_not_lookalikes() {
        assert!(is_wikipedia_url(Some("https://en.wikipedia.org/wiki/Rust")));
        assert!(is_wikipedia_url(Some("https://wikipedia.org/wiki/Rust")));
        assert!(is_wikipedia_url(Some(
            "https://en.wikipedia.org./wiki/Rust"
        )));
        assert!(!is_wikipedia_url(Some(
            "https://en.wikipedia.org.evil.example/wiki/Rust"
        )));
        assert!(!is_wikipedia_url(Some("https://notwikipedia.org/")));
        assert!(!is_wikipedia_url(Some("notaurl")));
        assert!(!is_wikipedia_url(None));
    }

    #[test]
    fn short_data_uris_stay_and_long_ones_shrink() {
        assert_eq!(
            truncate_data_uris("see data:text/plain,hi."),
            "see data:text/plain,hi."
        );
        let long = format!("data:image/png;base64,{}", "A".repeat(80));
        assert_eq!(truncate_data_uris(&long), "data:...");
        assert!(truncate_data_uris("metadata:text/plain,hi").contains("metadata:"));
    }
}
