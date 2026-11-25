use std::path::{Path, PathBuf};

use markitdown::{confirmation_message, save_markdown, OutputDestination, SaveError};
use serde::Serialize;

/// Result the drop panel renders. The command is a thin call into `save_markdown`.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SaveResponse {
    Written {
        path: String,
        markdown: String,
    },
    NeedsConfirmation {
        path: String,
        message: String,
        markdown: String,
    },
    Failed {
        message: String,
    },
}

/// A relative explicit path is joined to the source file's directory.
/// The panel's working directory is not a place the user chose.
fn explicit_from_panel(source: &Path, path: PathBuf) -> PathBuf {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return path;
    }
    match source.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join(path),
        _ => path,
    }
}

#[tauri::command]
pub(crate) fn save_conversion(
    source: String,
    destination: Option<String>,
    replace_existing: bool,
) -> SaveResponse {
    let source_path = PathBuf::from(&source);
    let destination = match destination {
        Some(path) => {
            OutputDestination::Explicit(explicit_from_panel(&source_path, PathBuf::from(path)))
        }
        None => OutputDestination::BesideSource,
    };
    match save_markdown(source_path.as_path(), &destination, replace_existing) {
        Ok(saved) => SaveResponse::Written {
            path: saved.path.display().to_string(),
            markdown: saved.markdown,
        },
        Err(SaveError::NeedsConfirmation { path, markdown }) => SaveResponse::NeedsConfirmation {
            path: path.display().to_string(),
            message: confirmation_message(&path),
            markdown,
        },
        Err(err) => SaveResponse::Failed {
            message: err.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use markitdown::MarkItDown;

    fn csv_dir() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("notes.csv");
        std::fs::write(&source, b"name,city\nAda,London\n").unwrap();
        (dir, source)
    }

    fn markdown_of(source: &Path) -> String {
        MarkItDown::new().convert_path(source).unwrap().markdown
    }

    #[test]
    fn panel_command_writes_beside_or_only_the_explicit_path() {
        let (dir, source) = csv_dir();
        let expected = markdown_of(&source);
        let beside = source.with_extension("md");

        let written = save_conversion(source.display().to_string(), None, false);
        let json = serde_json::to_value(&written).unwrap();
        assert_eq!(json["status"], "written");
        assert_eq!(json["path"], beside.display().to_string());
        assert_eq!(json["markdown"], expected);
        assert_eq!(std::fs::read_to_string(&beside).unwrap(), expected);
        std::fs::remove_file(&beside).unwrap();

        let chosen = dir.path().join("out").join("chosen.md");
        std::fs::create_dir(chosen.parent().unwrap()).unwrap();
        let written = save_conversion(
            source.display().to_string(),
            Some(chosen.display().to_string()),
            false,
        );
        let json = serde_json::to_value(&written).unwrap();
        assert_eq!(json["status"], "written");
        assert_eq!(json["path"], chosen.display().to_string());
        assert_eq!(json["markdown"], expected);
        assert!(!beside.exists());
        assert_eq!(std::fs::read_to_string(&chosen).unwrap(), expected);
    }

    #[test]
    fn panel_command_resolves_a_relative_path_next_to_the_source() {
        let (dir, source) = csv_dir();
        let expected = markdown_of(&source);
        let written = save_conversion(
            source.display().to_string(),
            Some("chosen.md".into()),
            false,
        );
        let json = serde_json::to_value(&written).unwrap();
        let resolved = dir.path().join("chosen.md");
        assert_eq!(json["status"], "written");
        assert_eq!(json["path"], resolved.display().to_string());
        assert_eq!(json["markdown"], expected);
        assert_eq!(std::fs::read_to_string(&resolved).unwrap(), expected);
        assert!(!source.with_extension("md").exists());

        let missing = save_conversion(
            source.display().to_string(),
            Some("no-such-dir/out.md".into()),
            false,
        );
        let json = serde_json::to_value(&missing).unwrap();
        assert_eq!(json["status"], "failed");
        assert!(json["message"].as_str().unwrap().contains("directory"));
        assert!(!dir.path().join("no-such-dir").exists());
    }

    #[test]
    fn panel_command_reports_confirmation_and_failure_without_writing() {
        let (dir, source) = csv_dir();
        let beside = source.with_extension("md");
        std::fs::write(&beside, "OLD").unwrap();
        let refused = save_conversion(source.display().to_string(), None, false);
        let json = serde_json::to_value(&refused).unwrap();
        assert_eq!(json["status"], "needsConfirmation");
        assert_eq!(json["path"], beside.display().to_string());
        assert_eq!(json["message"], confirmation_message(&beside));
        assert_eq!(json["markdown"], markdown_of(&source));
        assert_eq!(std::fs::read_to_string(&beside).unwrap(), "OLD");

        let replaced = save_conversion(source.display().to_string(), None, true);
        let json = serde_json::to_value(&replaced).unwrap();
        assert_eq!(json["status"], "written");
        assert_eq!(json["markdown"], markdown_of(&source));

        let empty = save_conversion(source.display().to_string(), Some(String::new()), false);
        let json = serde_json::to_value(&empty).unwrap();
        assert_eq!(json["status"], "failed");
        assert!(json["message"]
            .as_str()
            .unwrap()
            .contains("Choose an output path"));

        let blob = dir.path().join("blob.bin");
        std::fs::write(&blob, [0, 1, 2, 3, 4]).unwrap();
        let kept = dir.path().join("kept.md");
        std::fs::write(&kept, "KEEP").unwrap();
        let failed = save_conversion(
            blob.display().to_string(),
            Some(kept.display().to_string()),
            true,
        );
        let json = serde_json::to_value(&failed).unwrap();
        assert_eq!(json["status"], "failed");
        assert!(!json["message"].as_str().unwrap().is_empty());
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "KEEP");
    }
}
