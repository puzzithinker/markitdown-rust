use std::path::PathBuf;

use markitdown::{save_markdown, OutputDestination, SaveError};
use serde::Serialize;

/// Result the drop panel renders. The command is a thin call into `save_markdown`.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SaveResponse {
    Written { path: String, markdown: String },
    NeedsConfirmation { path: String, message: String },
    Failed { message: String },
}

#[tauri::command]
fn save_conversion(
    source: String,
    destination: Option<String>,
    replace_existing: bool,
) -> SaveResponse {
    let destination = match destination {
        Some(path) => OutputDestination::Explicit(PathBuf::from(path)),
        None => OutputDestination::BesideSource,
    };
    match save_markdown(
        PathBuf::from(&source).as_path(),
        &destination,
        replace_existing,
    ) {
        Ok(saved) => SaveResponse::Written {
            path: saved.path.display().to_string(),
            markdown: saved.markdown,
        },
        Err(SaveError::NeedsConfirmation { path }) => {
            let message = SaveError::NeedsConfirmation { path: path.clone() }.to_string();
            SaveResponse::NeedsConfirmation {
                path: path.display().to_string(),
                message,
            }
        }
        Err(err) => SaveResponse::Failed {
            message: err.to_string(),
        },
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|_app| {
            if let Ok(source) = std::env::var("MARKITDOWN_SMOKE_SOURCE") {
                let destination = std::env::var("MARKITDOWN_SMOKE_DEST").ok();
                let replace_existing =
                    std::env::var("MARKITDOWN_SMOKE_REPLACE").ok().as_deref() == Some("1");
                let response = save_conversion(source, destination, replace_existing);
                match serde_json::to_string(&response) {
                    Ok(json) => eprintln!("MARKITDOWN_SMOKE {json}"),
                    Err(err) => eprintln!("MARKITDOWN_SMOKE {err}"),
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![save_conversion])
        .run(tauri::generate_context!())
        .expect("error while running MarkItDown");
}
