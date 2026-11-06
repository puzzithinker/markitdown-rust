use std::fmt;
use std::path::{Path, PathBuf};

use crate::engine::MarkItDown;

/// Where a conversion is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputDestination {
    /// Same directory as the source, using the source stem and a `.md` suffix.
    BesideSource,
    /// This path only. The beside-source file is not created.
    Explicit(PathBuf),
}

/// Markdown that was written to disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedMarkdown {
    pub path: PathBuf,
    pub markdown: String,
}

/// Why saving Markdown did not write a file.
#[derive(Debug)]
pub enum SaveError {
    /// The source could not be converted. Nothing was written.
    Conversion(String),
    /// The destination already exists and replacement was not confirmed.
    NeedsConfirmation { path: PathBuf },
    /// The request or the write itself failed. Nothing new was left behind.
    Message(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Conversion(message) | SaveError::Message(message) => f.write_str(message),
            SaveError::NeedsConfirmation { path } => write!(
                f,
                "Destination already exists: {}. Confirm replacement to overwrite it.",
                path.display()
            ),
        }
    }
}

impl std::error::Error for SaveError {}

/// Path used by [`OutputDestination::BesideSource`].
pub fn beside_source_path(source: &Path) -> PathBuf {
    source.with_extension("md")
}

/// Convert `source` with [`MarkItDown`] and write the Markdown.
///
/// A failed conversion does not create or truncate the destination. An existing
/// destination is left unchanged unless `replace_existing` is true.
pub fn save_markdown(
    source: &Path,
    destination: &OutputDestination,
    replace_existing: bool,
) -> Result<SavedMarkdown, SaveError> {
    let dest = match destination {
        OutputDestination::BesideSource => beside_source_path(source),
        OutputDestination::Explicit(path) => {
            if path.as_os_str().is_empty() {
                return Err(SaveError::Message(
                    "Choose an output path before saving.".into(),
                ));
            }
            path.clone()
        }
    };
    if dest.is_dir() {
        return Err(SaveError::Message(format!(
            "{} is a directory, not a Markdown file",
            dest.display()
        )));
    }
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() && !parent.is_dir() {
            return Err(SaveError::Message(format!(
                "Output directory does not exist: {}",
                parent.display()
            )));
        }
    }

    let markdown = match MarkItDown::new().convert_path(source) {
        Ok(result) => result.markdown,
        Err(err) => return Err(SaveError::Conversion(err.to_string())),
    };

    if std::fs::symlink_metadata(&dest).is_ok() && !replace_existing {
        return Err(SaveError::NeedsConfirmation { path: dest });
    }

    let temporary = temporary_path(&dest);
    if let Err(err) = std::fs::write(&temporary, &markdown) {
        let _ = std::fs::remove_file(&temporary);
        return Err(SaveError::Message(err.to_string()));
    }
    if let Err(err) = std::fs::rename(&temporary, &dest) {
        let _ = std::fs::remove_file(&temporary);
        return Err(SaveError::Message(err.to_string()));
    }

    Ok(SavedMarkdown {
        path: dest,
        markdown,
    })
}

fn temporary_path(dest: &Path) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "markdown.md".into());
    name.push(format!(".markitdown-tmp-{}", std::process::id()));
    match dest.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}
