use std::path::Path;

/// Hints about an input stream. Every field is optional.
///
/// Extension, when present, includes the leading dot and is lowercase (`.pdf`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamInfo {
    pub mimetype: Option<String>,
    pub extension: Option<String>,
    pub charset: Option<String>,
    pub filename: Option<String>,
    pub local_path: Option<String>,
    pub url: Option<String>,
}

impl StreamInfo {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fill filename, extension, and local path from a filesystem path.
    pub fn from_path(path: &Path) -> Self {
        let mut info = Self::new();
        info.fill_missing_from_path(path);
        info
    }

    /// Fill filename, extension, and local path when those fields are empty.
    ///
    /// Hints the caller already set are left alone.
    pub(crate) fn fill_missing_from_path(&mut self, path: &Path) {
        if self.filename.is_none() {
            self.filename = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
        }
        if self.local_path.is_none() {
            self.local_path = Some(path.display().to_string());
        }
        if self.extension.is_none() {
            if let Some(extension) = path.extension() {
                self.extension = some_extension(&extension.to_string_lossy());
            }
        }
    }

    pub fn with_extension(mut self, extension: impl Into<String>) -> Self {
        self.extension = some_extension(&extension.into());
        self
    }

    pub fn with_mimetype(mut self, mimetype: impl Into<String>) -> Self {
        self.mimetype = Some(mimetype.into());
        self
    }

    pub fn with_charset(mut self, charset: impl Into<String>) -> Self {
        self.charset = Some(charset.into());
        self
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }
}

/// Lowercase an extension and give it a leading dot.
///
/// A blank suffix, or a suffix made only of dots, becomes an empty string.
pub fn normalize_extension(extension: &str) -> String {
    let trimmed = extension.trim().to_ascii_lowercase();
    if trimmed.is_empty() || trimmed.bytes().all(|byte| byte == b'.') {
        return String::new();
    }
    if trimmed.starts_with('.') {
        trimmed
    } else {
        format!(".{trimmed}")
    }
}

pub(crate) fn some_extension(extension: &str) -> Option<String> {
    let normalized = normalize_extension(extension);
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::{some_extension, StreamInfo};
    use std::path::Path;

    #[test]
    fn normalizes_case_and_rejects_a_dot_only_suffix() {
        assert_eq!(
            StreamInfo::new()
                .with_extension(" CSV ")
                .extension
                .as_deref(),
            Some(".csv")
        );
        assert_eq!(
            StreamInfo::new()
                .with_extension(".PDF")
                .extension
                .as_deref(),
            Some(".pdf")
        );
        assert_eq!(StreamInfo::new().with_extension(".").extension, None);
        assert_eq!(StreamInfo::new().with_extension("..").extension, None);
        assert_eq!(some_extension("md").as_deref(), Some(".md"));
    }

    #[test]
    fn from_path_fills_name_extension_and_local_path() {
        let info = StreamInfo::from_path(Path::new("/tmp/Notes.MD"));
        assert_eq!(info.filename.as_deref(), Some("Notes.MD"));
        assert_eq!(info.extension.as_deref(), Some(".md"));
        assert_eq!(info.local_path.as_deref(), Some("/tmp/Notes.MD"));
        assert_eq!(
            StreamInfo::from_path(Path::new("/tmp/.gitignore")).extension,
            None
        );
    }
}
