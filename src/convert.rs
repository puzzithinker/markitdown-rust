use std::path::Path;
use std::sync::Arc;

use crate::error::Result;
use crate::stream_info::StreamInfo;

/// Tried before generic converters such as HTML, ZIP, and plain text.
pub const PRIORITY_SPECIFIC: f32 = 0.0;
/// Catch-all converters. Lower values run first.
pub const PRIORITY_GENERIC: f32 = 10.0;

/// Markdown produced from one input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentConverterResult {
    pub markdown: String,
    pub title: Option<String>,
}

impl DocumentConverterResult {
    pub fn new(markdown: impl Into<String>) -> Self {
        Self {
            markdown: markdown.into(),
            title: None,
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        let title = title.into();
        self.title = if title.trim().is_empty() {
            None
        } else {
            Some(title)
        };
        self
    }
}

impl std::fmt::Display for DocumentConverterResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.markdown)
    }
}

/// Knobs for conversion. ZIP limits apply to archives and to archives inside them.
#[derive(Debug, Clone)]
pub struct Options {
    pub keep_data_uris: bool,
    pub max_zip_entries: usize,
    pub max_zip_uncompressed_bytes: u64,
    pub max_zip_depth: u32,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            keep_data_uris: false,
            max_zip_entries: 10_000,
            max_zip_uncompressed_bytes: 512 * 1024 * 1024,
            max_zip_depth: 3,
        }
    }
}

/// Context passed into a converter.
///
/// `path` is set only for the original filesystem input, so PDF and Office
/// converters can hand that path to unpdf and undoc. Nested ZIP members have
/// no path and are converted from bytes. `depth` counts archive nesting.
pub struct ConvertContext<'a> {
    pub engine: &'a crate::MarkItDown,
    pub options: &'a Options,
    pub path: Option<&'a Path>,
    pub depth: u32,
}

/// A format handler. Built-in converters and anything registered later share this.
pub trait Converter: Send + Sync {
    fn name(&self) -> &'static str;
    fn accepts(&self, info: &StreamInfo, bytes: &[u8]) -> bool;
    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult>;
}

pub type DynConverter = Arc<dyn Converter>;

pub(crate) struct Registration {
    pub converter: DynConverter,
    pub priority: f32,
    pub order: u32,
}

#[cfg(test)]
mod tests {
    use super::{DocumentConverterResult, Options};

    #[test]
    fn blank_title_is_absent_and_display_is_the_markdown() {
        let result = DocumentConverterResult::new("body\n").with_title("   ");
        assert_eq!(result.title, None);
        assert_eq!(result.to_string(), "body\n");
        let titled = DocumentConverterResult::new("body").with_title(" Title ");
        assert_eq!(titled.title.as_deref(), Some(" Title "));
    }

    #[test]
    fn default_limits_match_the_archive_budget() {
        let options = Options::default();
        assert!(!options.keep_data_uris);
        assert_eq!(options.max_zip_entries, 10_000);
        assert_eq!(options.max_zip_uncompressed_bytes, 512 * 1024 * 1024);
        assert_eq!(options.max_zip_depth, 3);
    }
}
