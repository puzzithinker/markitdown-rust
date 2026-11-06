use std::path::Path;

use markitdown::{beside_source_path, save_markdown, MarkItDown, OutputDestination, SaveError};

fn csv_bytes() -> &'static [u8] {
    b"name,city\nAda,London\n"
}

fn write_source(dir: &Path, name: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, csv_bytes()).unwrap();
    path
}

fn library_markdown(source: &Path) -> String {
    MarkItDown::new().convert_path(source).unwrap().markdown
}

fn names(dir: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    names
}

#[test]
fn beside_source_matches_the_library_and_writes_only_that_file() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), "notes.csv");
    let expected = library_markdown(&source);
    let saved = save_markdown(&source, &OutputDestination::BesideSource, false).unwrap();
    let beside = beside_source_path(&source);
    assert_eq!(saved.path, beside);
    assert_eq!(saved.markdown, expected);
    assert_eq!(std::fs::read_to_string(&beside).unwrap(), expected);
    assert_eq!(
        names(dir.path()),
        vec!["notes.csv".to_string(), "notes.md".to_string()]
    );
}

#[test]
fn explicit_path_does_not_also_write_beside_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), "notes.csv");
    let explicit = dir.path().join("out").join("chosen.md");
    std::fs::create_dir(explicit.parent().unwrap()).unwrap();
    let expected = library_markdown(&source);
    let saved = save_markdown(
        &source,
        &OutputDestination::Explicit(explicit.clone()),
        false,
    )
    .unwrap();
    assert_eq!(saved.path, explicit);
    assert_eq!(saved.markdown, expected);
    assert_eq!(std::fs::read_to_string(&explicit).unwrap(), expected);
    assert!(!beside_source_path(&source).exists());
    assert_eq!(
        names(dir.path()),
        vec!["notes.csv".to_string(), "out".to_string()]
    );
}

#[test]
fn failed_conversion_writes_nothing_and_returns_a_readable_error() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("blob.bin");
    std::fs::write(&source, [0, 1, 2, 3, 4]).unwrap();
    let existing = beside_source_path(&source);
    std::fs::write(&existing, "KEEP").unwrap();
    let explicit = dir.path().join("elsewhere.md");
    std::fs::write(&explicit, "ALSO-KEEP").unwrap();

    let beside_err = save_markdown(&source, &OutputDestination::BesideSource, true).unwrap_err();
    let message = beside_err.to_string();
    assert!(!message.is_empty(), "{message}");
    assert!(!message.contains("SaveError"), "{message}");
    assert!(matches!(beside_err, SaveError::Conversion(_)));
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "KEEP");

    let explicit_err = save_markdown(
        &source,
        &OutputDestination::Explicit(explicit.clone()),
        true,
    )
    .unwrap_err();
    assert!(matches!(explicit_err, SaveError::Conversion(_)));
    assert!(!explicit_err.to_string().is_empty());
    assert_eq!(std::fs::read_to_string(&explicit).unwrap(), "ALSO-KEEP");
    assert_eq!(
        names(dir.path()),
        vec![
            "blob.bin".to_string(),
            "blob.md".to_string(),
            "elsewhere.md".to_string()
        ]
    );
}

#[test]
fn existing_destination_stays_until_replacement_is_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), "notes.csv");
    let expected = library_markdown(&source);

    let beside = beside_source_path(&source);
    std::fs::write(&beside, "OLD-BESIDE").unwrap();
    let refused = save_markdown(&source, &OutputDestination::BesideSource, false).unwrap_err();
    match &refused {
        SaveError::NeedsConfirmation { path } => assert_eq!(path, &beside),
        other => panic!("expected confirmation, got {other}"),
    }
    assert!(refused.to_string().contains("Confirm replacement"));
    assert_eq!(std::fs::read_to_string(&beside).unwrap(), "OLD-BESIDE");
    let replaced = save_markdown(&source, &OutputDestination::BesideSource, true).unwrap();
    assert_eq!(replaced.markdown, expected);
    assert_eq!(std::fs::read_to_string(&beside).unwrap(), expected);

    let explicit = dir.path().join("chosen.md");
    std::fs::write(&explicit, "OLD-EXPLICIT").unwrap();
    let refused = save_markdown(
        &source,
        &OutputDestination::Explicit(explicit.clone()),
        false,
    )
    .unwrap_err();
    assert!(matches!(refused, SaveError::NeedsConfirmation { .. }));
    assert_eq!(std::fs::read_to_string(&explicit).unwrap(), "OLD-EXPLICIT");
    assert_eq!(std::fs::read_to_string(&beside).unwrap(), expected);
    let replaced = save_markdown(
        &source,
        &OutputDestination::Explicit(explicit.clone()),
        true,
    )
    .unwrap();
    assert_eq!(replaced.markdown, expected);
    assert_eq!(std::fs::read_to_string(&explicit).unwrap(), expected);
    assert_eq!(std::fs::read_to_string(&beside).unwrap(), expected);
}

#[test]
fn missing_source_and_bad_destination_do_not_create_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.csv");
    let err = save_markdown(&missing, &OutputDestination::BesideSource, false).unwrap_err();
    assert!(matches!(err, SaveError::Conversion(_)));
    assert!(!err.to_string().is_empty());
    assert!(!beside_source_path(&missing).exists());

    let source = write_source(dir.path(), "notes.csv");
    let err = save_markdown(
        &source,
        &OutputDestination::Explicit(dir.path().into()),
        false,
    )
    .unwrap_err();
    assert!(matches!(err, SaveError::Message(_)));
    assert!(err.to_string().contains("directory"));
    let missing_parent = dir.path().join("no-such-dir").join("out.md");
    let err = save_markdown(
        &source,
        &OutputDestination::Explicit(missing_parent.clone()),
        false,
    )
    .unwrap_err();
    assert!(matches!(err, SaveError::Message(_)));
    assert!(!missing_parent.exists());
    assert!(!beside_source_path(&source).exists());
}
