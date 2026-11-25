use std::path::Path;

use markitdown::{
    beside_source_path, confirmation_message, save_markdown, MarkItDown, OutputDestination,
    SaveError,
};

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
        SaveError::NeedsConfirmation { path, markdown } => {
            assert_eq!(path, &beside);
            assert_eq!(markdown, &expected);
        }
        other => panic!("expected confirmation, got {other}"),
    }
    assert_eq!(refused.to_string(), confirmation_message(&beside));
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
    let names = names(dir.path());
    assert!(
        names
            .iter()
            .all(|name| !name.contains("markitdown-bak") && !name.contains("markitdown-tmp")),
        "{names:?}"
    );
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

#[test]
fn beside_source_replaces_only_the_last_suffix() {
    assert_eq!(
        beside_source_path(Path::new("archive.tar.gz")),
        Path::new("archive.tar.md")
    );
    assert_eq!(
        beside_source_path(Path::new("README")),
        Path::new("README.md")
    );
    assert_eq!(
        beside_source_path(Path::new(".gitignore")),
        Path::new(".gitignore.md")
    );

    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), "archive.tar.gz");
    let saved = save_markdown(&source, &OutputDestination::BesideSource, false).unwrap();
    assert_eq!(saved.path, dir.path().join("archive.tar.md"));
    assert_eq!(
        std::fs::read_to_string(&saved.path).unwrap(),
        library_markdown(&source)
    );
    assert!(!dir.path().join("archive.md").exists());
    assert!(!dir.path().join("archive.tar.gz.md").exists());
}

#[test]
fn empty_explicit_path_and_a_blocked_parent_write_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), "notes.csv");
    let err = save_markdown(
        &source,
        &OutputDestination::Explicit(std::path::PathBuf::new()),
        false,
    )
    .unwrap_err();
    assert!(matches!(err, SaveError::Message(_)));
    assert!(err.to_string().contains("Choose an output path"));
    assert!(!beside_source_path(&source).exists());

    let blocker = dir.path().join("not-a-directory");
    std::fs::write(&blocker, b"x").unwrap();
    let dest = blocker.join("out.md");
    let err =
        save_markdown(&source, &OutputDestination::Explicit(dest.clone()), false).unwrap_err();
    assert!(matches!(err, SaveError::Message(_)));
    assert!(err.to_string().contains("directory"));
    assert!(!dest.exists());
    assert!(!beside_source_path(&source).exists());
}

#[test]
fn a_directory_source_and_an_existing_markdown_source_do_not_truncate() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("folder");
    std::fs::create_dir(&folder).unwrap();
    let err = save_markdown(&folder, &OutputDestination::BesideSource, false).unwrap_err();
    assert!(matches!(err, SaveError::Conversion(_)));
    assert!(!err.to_string().is_empty());
    assert!(!dir.path().join("folder.md").exists());

    let source = dir.path().join("notes.md");
    let original = b"hello  \n\n\n\nworld  \n";
    std::fs::write(&source, original).unwrap();
    let err = save_markdown(&source, &OutputDestination::BesideSource, false).unwrap_err();
    match &err {
        SaveError::NeedsConfirmation { path, markdown } => {
            assert_eq!(path, &source);
            assert_eq!(markdown, &library_markdown(&source));
        }
        other => panic!("expected confirmation, got {other}"),
    }
    assert_eq!(std::fs::read(&source).unwrap(), original);
    let replaced = save_markdown(&source, &OutputDestination::BesideSource, true).unwrap();
    assert_eq!(replaced.path, source);
    assert_eq!(std::fs::read_to_string(&source).unwrap(), replaced.markdown);
    assert_ne!(replaced.markdown.as_bytes(), original);
}

#[cfg(unix)]
#[test]
fn a_symlink_destination_stays_until_replacement_is_confirmed() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), "notes.csv");
    let expected = library_markdown(&source);
    let keep = dir.path().join("keep.txt");
    std::fs::write(&keep, "KEEP").unwrap();
    let dest = dir.path().join("chosen.md");
    std::os::unix::fs::symlink(&keep, &dest).unwrap();

    let err =
        save_markdown(&source, &OutputDestination::Explicit(dest.clone()), false).unwrap_err();
    assert!(matches!(err, SaveError::NeedsConfirmation { .. }));
    assert_eq!(std::fs::read_to_string(&keep).unwrap(), "KEEP");
    assert!(std::fs::symlink_metadata(&dest)
        .unwrap()
        .file_type()
        .is_symlink());

    let dangling = dir.path().join("dangling.md");
    std::os::unix::fs::symlink("missing-target", &dangling).unwrap();
    let err = save_markdown(
        &source,
        &OutputDestination::Explicit(dangling.clone()),
        false,
    )
    .unwrap_err();
    assert!(matches!(err, SaveError::NeedsConfirmation { .. }));
    assert!(std::fs::symlink_metadata(&dangling)
        .unwrap()
        .file_type()
        .is_symlink());

    let saved = save_markdown(&source, &OutputDestination::Explicit(dest.clone()), true).unwrap();
    assert_eq!(saved.markdown, expected);
    assert!(std::fs::metadata(&dest).unwrap().is_file());
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), expected);
    assert_eq!(std::fs::read_to_string(&keep).unwrap(), "KEEP");
    assert!(!beside_source_path(&source).exists());
}

#[test]
fn overlapping_replacements_keep_a_complete_markdown_file() {
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let source_a = write_source(dir.path(), "a.csv");
    let source_b = dir.path().join("b.csv");
    std::fs::write(&source_b, b"name,city\nGrace,Paris\n").unwrap();
    let expected_a = library_markdown(&source_a);
    let expected_b = library_markdown(&source_b);
    assert_ne!(expected_a, expected_b);
    let dest = dir.path().join("out.md");
    std::fs::write(&dest, "OLD").unwrap();

    let dest_a = dest.clone();
    let dest_b = dest.clone();
    let left =
        thread::spawn(move || save_markdown(&source_a, &OutputDestination::Explicit(dest_a), true));
    let right =
        thread::spawn(move || save_markdown(&source_b, &OutputDestination::Explicit(dest_b), true));
    let left = left.join().expect("left save");
    let right = right.join().expect("right save");
    if let Ok(saved) = &left {
        assert_eq!(saved.markdown, expected_a);
    }
    if let Ok(saved) = &right {
        assert_eq!(saved.markdown, expected_b);
    }
    assert!(
        left.is_ok() || right.is_ok(),
        "both overlapping saves failed: {left:?} {right:?}"
    );
    let body = std::fs::read_to_string(&dest).unwrap();
    assert!(
        body == expected_a || body == expected_b,
        "destination became {body:?}"
    );
    let names = names(dir.path());
    assert!(
        names
            .iter()
            .all(|name| !name.contains("markitdown-bak") && !name.contains("markitdown-tmp")),
        "{names:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_read_only_destination_file_is_replaced_and_no_backup_remains() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), "notes.csv");
    let dest = dir.path().join("chosen.md");
    std::fs::write(&dest, "OLD").unwrap();
    let mut perms = std::fs::metadata(&dest).unwrap().permissions();
    perms.set_readonly(true);
    std::fs::set_permissions(&dest, perms).unwrap();

    let saved = save_markdown(&source, &OutputDestination::Explicit(dest.clone()), true).unwrap();
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), saved.markdown);
    assert_ne!(saved.markdown, "OLD");
    let names = names(dir.path());
    assert!(
        names
            .iter()
            .all(|name| !name.contains("markitdown-bak") && !name.contains("markitdown-tmp")),
        "{names:?}"
    );
}

#[cfg(unix)]
#[test]
fn an_unwritable_directory_leaves_no_temporary_file() {
    let dir = tempfile::tempdir().unwrap();
    let source = write_source(dir.path(), "notes.csv");
    let parent = dir.path().join("sealed");
    std::fs::create_dir(&parent).unwrap();
    let dest = parent.join("out.md");
    let original = std::fs::metadata(&parent).unwrap().permissions();
    let mut readonly = original.clone();
    readonly.set_readonly(true);
    std::fs::set_permissions(&parent, readonly).unwrap();

    let result = save_markdown(&source, &OutputDestination::Explicit(dest.clone()), false);

    std::fs::set_permissions(&parent, original).unwrap();

    match result {
        Err(SaveError::Message(_)) => {
            assert!(!dest.exists());
            let leftovers: Vec<_> = std::fs::read_dir(&parent)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert!(leftovers.is_empty(), "{leftovers:?}");
            assert!(!beside_source_path(&source).exists());
        }
        Ok(_) => {
            // A privileged process can ignore the mode bits. The write still
            // has to be the Markdown, with no temporary name left behind.
            assert_eq!(
                std::fs::read_to_string(&dest).unwrap(),
                library_markdown(&source)
            );
            let names = names(&parent);
            assert_eq!(names, vec!["out.md".to_string()]);
        }
        Err(other) => panic!("expected a write error, got {other}"),
    }
}
