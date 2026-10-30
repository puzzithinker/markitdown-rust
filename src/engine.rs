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
    converters: Vec<Registration>,
    options: Options,
    next_order: u32,
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
            next_order: 0,
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
        self.next_order = self.next_order.saturating_add(1);
        self.converters.push(Registration {
            converter,
            priority,
            order: self.next_order,
        });
    }

    pub fn convert_path(&self, path: impl AsRef<Path>) -> Result<DocumentConverterResult> {
        let path = path.as_ref();
        self.convert_path_with_info(path, StreamInfo::from_path(path))
    }

    /// Convert a filesystem path, using `info` for extension, type, and charset hints.
    ///
    /// Missing filename and local path are filled from `path`. The path is passed
    /// through so PDF and Office readers can open the file directly.
    pub fn convert_path_with_info(
        &self,
        path: impl AsRef<Path>,
        mut info: StreamInfo,
    ) -> Result<DocumentConverterResult> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)?;
        if info.filename.is_none() {
            info.filename = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
        }
        if info.local_path.is_none() {
            info.local_path = Some(path.display().to_string());
        }
        if info.extension.is_none() {
            if let Some(extension) = path.extension() {
                info.extension = crate::stream_info::some_extension(&extension.to_string_lossy());
            }
        }
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
        let mut order: Vec<(f32, u32, usize)> = self
            .converters
            .iter()
            .enumerate()
            .map(|(index, registration)| (registration.priority, registration.order, index))
            .collect();
        order.sort_by(|left, right| {
            left.0
                .total_cmp(&right.0)
                .then_with(|| right.1.cmp(&left.1))
        });
        let scheduled: Vec<DynConverter> = order
            .into_iter()
            .map(|(_, _, index)| Arc::clone(&self.converters[index].converter))
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
