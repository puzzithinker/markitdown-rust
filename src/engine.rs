use std::path::Path;
use std::sync::Arc;

use crate::convert::{
    ConvertContext, Converter, DocumentConverterResult, DynConverter, Options, Registration,
};
use crate::converters;
use crate::detect::build_guesses;
use crate::error::{Error, FailedAttempt, Result};
use crate::normalize::normalize_markdown;
use crate::stream_info::StreamInfo;

/// Converts files, bytes, and URIs to Markdown.
pub struct MarkItDown {
    /// Run order: lower priority first, and within one priority the newest first.
    converters: Vec<Registration>,
    options: Options,
}

impl Default for MarkItDown {
    fn default() -> Self {
        Self::new()
    }
}

impl MarkItDown {
    pub fn new() -> Self {
        Self::with_options(Options::default())
    }

    pub fn with_options(options: Options) -> Self {
        let mut engine = Self {
            converters: Vec::new(),
            options,
        };
        for (converter, priority) in converters::builtins() {
            engine.register_arc(converter, priority);
        }
        engine
    }

    pub fn options(&self) -> &Options {
        &self.options
    }

    pub fn options_mut(&mut self) -> &mut Options {
        &mut self.options
    }

    /// Register a converter. A lower `priority` runs first. Within one priority,
    /// the most recently registered converter runs first.
    pub fn register<C>(&mut self, converter: C, priority: f32)
    where
        C: Converter + 'static,
    {
        self.register_arc(Arc::new(converter), priority);
    }

    pub fn register_arc(&mut self, converter: DynConverter, priority: f32) {
        // The first entry at this priority, or the first one after it. Putting
        // the new converter there leaves older converters of the same priority behind it.
        let index = self
            .converters
            .iter()
            .position(|existing| existing.priority.total_cmp(&priority).is_ge())
            .unwrap_or(self.converters.len());
        self.converters.insert(
            index,
            Registration {
                converter,
                priority,
            },
        );
    }

    pub fn convert_path(&self, path: impl AsRef<Path>) -> Result<DocumentConverterResult> {
        let path = path.as_ref();
        self.convert_path_with_info(path, StreamInfo::from_path(path))
    }

    /// Convert a filesystem path, using `info` for extension, type, and charset hints.
    ///
    /// Missing filename, extension, and local path are filled from `path`. The path is passed
    /// through so PDF and Office readers can open the file directly.
    pub fn convert_path_with_info(
        &self,
        path: impl AsRef<Path>,
        mut info: StreamInfo,
    ) -> Result<DocumentConverterResult> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        info.fill_missing_from_path(path);
        self.convert_inner(&bytes, info, Some(path), 0)
    }

    pub fn convert_bytes(&self, bytes: &[u8], info: StreamInfo) -> Result<DocumentConverterResult> {
        self.convert_inner(bytes, info, None, 0)
    }

    pub fn convert_reader(
        &self,
        mut reader: impl std::io::Read,
        info: StreamInfo,
    ) -> Result<DocumentConverterResult> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        self.convert_bytes(&bytes, info)
    }

    pub fn convert_uri(&self, uri: &str) -> Result<DocumentConverterResult> {
        crate::fetch::convert_uri(self, uri)
    }

    pub(crate) fn convert_inner(
        &self,
        bytes: &[u8],
        info: StreamInfo,
        path: Option<&Path>,
        depth: u32,
    ) -> Result<DocumentConverterResult> {
        let guesses = build_guesses(bytes, info);
        // A ZIP member is converted through this same engine. The list is copied
        // first so that nested call does not borrow `converters` again.
        let scheduled: Vec<DynConverter> = self
            .converters
            .iter()
            .map(|registration| Arc::clone(&registration.converter))
            .collect();

        let mut attempts = Vec::new();
        for guess in &guesses {
            for converter in &scheduled {
                if !converter.accepts(guess, bytes) {
                    continue;
                }
                let ctx = ConvertContext {
                    engine: self,
                    options: &self.options,
                    path,
                    depth,
                };
                match converter.convert(bytes, guess, &ctx) {
                    Ok(mut result) => {
                        result.markdown = normalize_markdown(&result.markdown);
                        return Ok(result);
                    }
                    Err(error) => attempts.push(FailedAttempt {
                        converter: converter.name(),
                        message: error.to_string(),
                    }),
                }
            }
        }

        if attempts.is_empty() {
            Err(Error::UnsupportedFormat(
                "Could not convert stream to Markdown. No converter attempted a conversion, suggesting that the filetype is not supported.".to_string(),
            ))
        } else {
            Err(Error::FileConversion(attempts))
        }
    }
}
