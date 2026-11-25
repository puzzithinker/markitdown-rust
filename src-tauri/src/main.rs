mod command;

use command::save_conversion;

/// The panel test reads this file. Stderr is shared with the window's own logs,
/// so a line there can be torn apart before the test sees a whole JSON value.
fn report_smoke(json: &str) {
    if let Ok(path) = std::env::var("MARKITDOWN_SMOKE_FILE") {
        let path = std::path::PathBuf::from(path);
        let mut partial = path.as_os_str().to_os_string();
        partial.push(".partial");
        let partial = std::path::PathBuf::from(partial);
        if let Err(err) =
            std::fs::write(&partial, json).and_then(|_| std::fs::rename(&partial, &path))
        {
            eprintln!("MARKITDOWN_SMOKE file: {err}");
        }
    }
    eprintln!("MARKITDOWN_SMOKE {json}");
}

fn smoke_from_env() {
    let source = match std::env::var("MARKITDOWN_SMOKE_SOURCE") {
        Ok(source) => source,
        Err(_) => return,
    };
    let destination = std::env::var("MARKITDOWN_SMOKE_DEST").ok();
    let replace_existing = std::env::var("MARKITDOWN_SMOKE_REPLACE").ok().as_deref() == Some("1");
    let response = save_conversion(source, destination, replace_existing);
    match serde_json::to_string(&response) {
        Ok(json) => report_smoke(&json),
        Err(err) => eprintln!("MARKITDOWN_SMOKE {err}"),
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|_app| {
            smoke_from_env();
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![save_conversion])
        .run(tauri::generate_context!())
        .expect("error while running MarkItDown");
}
