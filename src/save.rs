use std::fmt;
use std::path::{Path, PathBuf};

use crate::engine::MarkItDown;

/// Where a conversion is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputDestination {
    /// Same directory as the source, using the source stem and a `.md` suffix.
    ///
    /// `notes.csv` becomes `notes.md`. `archive.tar.gz` becomes `archive.tar.md`
    /// because only the last suffix is replaced. A source that is already `.md`
    /// targets itself, so replacement still has to be confirmed.
    BesideSource,
    /// This path only. The beside-source file is not created.
    ///
    /// A relative path is used as given. The desktop command resolves one
    /// against the source directory before calling this function.
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
    /// `markdown` is the conversion that would be written.
    NeedsConfirmation { path: PathBuf, markdown: String },
    /// The request or the write itself failed. Nothing new was left behind.
    Message(String),
}

impl fmt::Display for SaveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SaveError::Conversion(message) | SaveError::Message(message) => f.write_str(message),
            SaveError::NeedsConfirmation { path, .. } => f.write_str(&confirmation_message(path)),
        }
    }
}

/// Sentence shown when a destination already exists and replacement was not confirmed.
pub fn confirmation_message(path: &Path) -> String {
    format!(
        "Destination already exists: {}. Confirm replacement to overwrite it.",
        path.display()
    )
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
    let dest = resolve_destination(source, destination)?;
    let markdown = convert_source(source)?;
    if std::fs::symlink_metadata(&dest).is_ok() && !replace_existing {
        return Err(SaveError::NeedsConfirmation {
            path: dest,
            markdown,
        });
    }
    write_replaced(&dest, &markdown)?;
    Ok(SavedMarkdown {
        path: dest,
        markdown,
    })
}

fn resolve_destination(
    source: &Path,
    destination: &OutputDestination,
) -> Result<PathBuf, SaveError> {
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
    Ok(dest)
}

fn convert_source(source: &Path) -> Result<String, SaveError> {
    match MarkItDown::new().convert_path(source) {
        Ok(result) => Ok(result.markdown),
        Err(err) => Err(SaveError::Conversion(err.to_string())),
    }
}

fn write_replaced(dest: &Path, markdown: &str) -> Result<(), SaveError> {
    let temporary = temporary_path(dest);
    if let Err(err) = std::fs::write(&temporary, markdown) {
        let _ = std::fs::remove_file(&temporary);
        return Err(SaveError::Message(err.to_string()));
    }
    place_file(&temporary, dest).map_err(SaveError::Message)
}

fn temporary_path(dest: &Path) -> PathBuf {
    sibling_path(dest, &unique_suffix("markitdown-tmp"))
}

/// `rename` replaces an existing file on Unix and fails on Windows when the
/// destination already exists. Move the old file aside, move the new file into
/// place, and put the old file back if the second move fails.
fn place_file(temporary: &Path, dest: &Path) -> Result<(), String> {
    if std::fs::symlink_metadata(dest).is_err() {
        return std::fs::rename(temporary, dest).map_err(|err| {
            let _ = std::fs::remove_file(temporary);
            err.to_string()
        });
    }

    let backup = sibling_path(dest, &unique_suffix("markitdown-bak"));
    if let Err(err) = std::fs::rename(dest, &backup) {
        let _ = std::fs::remove_file(temporary);
        return Err(err.to_string());
    }
    if let Err(err) = std::fs::rename(temporary, dest) {
        let restored = std::fs::rename(&backup, dest);
        let _ = std::fs::remove_file(temporary);
        if let Err(restore) = restored {
            return Err(format!(
                "{err}. The previous file is at {}. Restoring it failed: {restore}",
                backup.display()
            ));
        }
        return Err(err.to_string());
    }
    let _ = std::fs::remove_file(&backup);
    Ok(())
}

/// The desktop process can save twice at once. A pid-only name would make those
/// saves share one temporary file and one backup.
fn unique_suffix(tag: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{tag}-{}-{nanos}-{n}", std::process::id())
}

fn sibling_path(dest: &Path, suffix: &str) -> PathBuf {
    let mut name = dest
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "markdown.md".into());
    name.push(".");
    name.push(suffix);
    match dest.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(name),
        _ => PathBuf::from(name),
    }
}
