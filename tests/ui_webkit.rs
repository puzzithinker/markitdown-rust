use std::process::Command;

/// Loads `ui/index.html` in WebKitGTK and drives the shipped panel.
/// Exit 77 means this machine has no display or no WebKit bindings.
#[test]
fn webkit_panel_matches_the_shipped_page() {
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/ui_webkit.py");
    let output = match Command::new("python3").arg(&script).output() {
        Ok(output) => output,
        Err(err) => {
            eprintln!("skipping webkit panel: {err}");
            return;
        }
    };
    if output.status.code() == Some(77) {
        eprintln!(
            "skipping webkit panel: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    assert!(
        output.status.success(),
        "webkit panel failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
