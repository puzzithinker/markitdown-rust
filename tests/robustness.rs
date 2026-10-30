use std::io::{Cursor, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

use markitdown::{
    ConvertContext, Converter, DocumentConverterResult, Error, MarkItDown, Options, StreamInfo,
    PRIORITY_SPECIFIC,
};

fn zip_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, bytes) in files {
        writer.start_file(*name, options).unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

struct Order {
    label: &'static str,
    log: Arc<Mutex<Vec<&'static str>>>,
    fail: bool,
    body: &'static str,
}

impl Converter for Order {
    fn name(&self) -> &'static str {
        self.label
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        info.extension.as_deref() == Some(".mine")
    }

    fn convert(
        &self,
        _bytes: &[u8],
        _info: &StreamInfo,
        _ctx: &ConvertContext<'_>,
    ) -> markitdown::Result<DocumentConverterResult> {
        self.log.lock().expect("log").push(self.label);
        if self.fail {
            Err(Error::Message("boom".into()))
        } else {
            Ok(DocumentConverterResult::new(self.body))
        }
    }
}

#[test]
fn registration_order_fallback_and_normalization() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let mut engine = MarkItDown::new();
    engine.register(
        Order {
            label: "second",
            log: Arc::clone(&log),
            fail: false,
            body: "a\n\n\n\nb  ",
        },
        1.0,
    );
    engine.register(
        Order {
            label: "first",
            log: Arc::clone(&log),
            fail: true,
            body: "unused",
        },
        1.0,
    );
    let result = engine
        .convert_bytes(b"payload", StreamInfo::new().with_extension(".mine"))
        .unwrap();
    assert_eq!(result.markdown, "a\n\nb");
    assert_eq!(log.lock().unwrap().as_slice(), ["first", "second"]);

    let mut priority = MarkItDown::new();
    let quiet = Arc::new(Mutex::new(Vec::new()));
    priority.register(
        Order {
            label: "later",
            log: Arc::clone(&quiet),
            fail: false,
            body: "later",
        },
        5.0,
    );
    priority.register(
        Order {
            label: "earlier",
            log: quiet,
            fail: false,
            body: "earlier",
        },
        0.0,
    );
    let result = priority
        .convert_bytes(b"payload", StreamInfo::new().with_extension(".mine"))
        .unwrap();
    assert_eq!(result.markdown, "earlier");
}

#[test]
fn a_failing_csv_converter_still_reaches_the_builtin() {
    struct Boom;
    impl Converter for Boom {
        fn name(&self) -> &'static str {
            "boom"
        }
        fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
            info.extension.as_deref() == Some(".csv")
        }
        fn convert(
            &self,
            _bytes: &[u8],
            _info: &StreamInfo,
            _ctx: &ConvertContext<'_>,
        ) -> markitdown::Result<DocumentConverterResult> {
            Err(Error::Message("boom".into()))
        }
    }

    let mut engine = MarkItDown::new();
    engine.register(Boom, PRIORITY_SPECIFIC);
    let result = engine
        .convert_bytes(
            b"name,city\nAda,London\n",
            StreamInfo::new().with_extension("csv"),
        )
        .unwrap();
    assert!(result.markdown.contains("| Ada | London |"));
}

#[test]
fn csv_edges_header_blanks_newlines_and_mime() {
    let engine = MarkItDown::new();
    let header = engine
        .convert_bytes(b"h1,h2\n", StreamInfo::new().with_extension(".csv"))
        .unwrap();
    assert_eq!(header.markdown, "| h1 | h2 |\n| --- | --- |");

    let empty = engine
        .convert_bytes(b"\n\n", StreamInfo::new().with_extension(".csv"))
        .unwrap();
    assert_eq!(empty.markdown, "");

    // The csv reader drops a physical blank line. A row of empty fields is real
    // and stays when it sits between data rows.
    let blank_line = engine
        .convert_bytes(b"h\n1\n\n2\n", StreamInfo::new().with_extension(".csv"))
        .unwrap();
    assert_eq!(blank_line.markdown, "| h |\n| --- |\n| 1 |\n| 2 |");

    let middle = engine
        .convert_bytes(
            b"h1,h2\n1,2\n,\n3,4\n",
            StreamInfo::new().with_extension(".csv"),
        )
        .unwrap();
    assert_eq!(
        middle.markdown,
        "| h1 | h2 |\n| --- | --- |\n| 1 | 2 |\n|  |  |\n| 3 | 4 |"
    );

    let under_header = engine
        .convert_bytes(
            b"h1,h2\n,\n1,2\n,\n",
            StreamInfo::new().with_extension(".csv"),
        )
        .unwrap();
    assert_eq!(
        under_header.markdown,
        "| h1 | h2 |\n| --- | --- |\n| 1 | 2 |"
    );

    let wrapped = engine
        .convert_bytes(b"h\n\"a\r\nb\"\n", StreamInfo::new().with_extension(".csv"))
        .unwrap();
    assert_eq!(wrapped.markdown, "| h |\n| --- |\n| a b |");

    let mime = engine
        .convert_bytes(
            b"a,b\n1,2\n",
            StreamInfo::new().with_mimetype("text/csv; charset=utf-8"),
        )
        .unwrap();
    assert!(mime.markdown.contains("| 1 | 2 |"));

    // An open quote runs to EOF, and a NUL inside a field stays in the cell.
    let open_quote = engine
        .convert_bytes(b"\"unterminated", StreamInfo::new().with_extension(".csv"))
        .unwrap();
    assert_eq!(open_quote.markdown, "| unterminated |\n| --- |");
    let with_nul = engine
        .convert_bytes(b"h\na\0b\n", StreamInfo::new().with_extension(".csv"))
        .unwrap();
    assert!(with_nul.markdown.contains("a\0b"), "{}", with_nul.markdown);
}

#[test]
fn html_noscript_xhtml_and_short_data_uris() {
    let html = b"<html><body><noscript>SECRET_NOSCRIPT</noscript><p>Visible</p>\
        <a href=\"data:text/plain,hi\">short</a></body></html>";
    let result = MarkItDown::new()
        .convert_bytes(html, StreamInfo::new().with_extension(".html"))
        .unwrap();
    assert!(result.markdown.contains("Visible"));
    assert!(!result.markdown.contains("SECRET_NOSCRIPT"));
    assert!(result.markdown.contains("data:text/plain,hi"));

    let xhtml = br#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>X Title</title></head>
<body><p>XHTML body</p></body></html>"#;
    let result = MarkItDown::new()
        .convert_bytes(xhtml, StreamInfo::new().with_extension(".xhtml"))
        .unwrap();
    assert_eq!(result.title.as_deref(), Some("X Title"));
    assert!(result.markdown.contains("XHTML body"));
}

#[test]
fn wikipedia_lookalikes_keep_chrome() {
    let html = r#"<html><body><div id="mw-content-text">
        <p>Article</p><div class="navbox">Chrome</div></div></body></html>"#;
    let engine = MarkItDown::new();
    for url in [
        "https://wikipedia.org/wiki/Rust",
        "https://en.wikipedia.org./wiki/Rust",
    ] {
        let page = engine
            .convert_bytes(
                html.as_bytes(),
                StreamInfo::new().with_extension(".html").with_url(url),
            )
            .unwrap();
        assert!(page.markdown.contains("Article"), "{url}");
        assert!(!page.markdown.contains("Chrome"), "{url}");
    }
    let lookalike = engine
        .convert_bytes(
            html.as_bytes(),
            StreamInfo::new()
                .with_extension(".html")
                .with_url("https://en.wikipedia.org.evil.example/wiki/Rust"),
        )
        .unwrap();
    assert!(lookalike.markdown.contains("Chrome"));
}

#[test]
fn notebooks_feeds_and_plain_passthrough() {
    let notebook = r##"{"nbformat":4,"cells":[{"cell_type":"markdown","source":["# From heading\n","Body"]}]}"##;
    let result = MarkItDown::new()
        .convert_bytes(
            notebook.as_bytes(),
            StreamInfo::new().with_mimetype("application/json"),
        )
        .unwrap();
    assert_eq!(result.title.as_deref(), Some("From heading"));
    assert!(result.markdown.contains("Body"));

    // `.ipynb` is also `application/json`, so a parse failure falls through to plain text.
    let raw = MarkItDown::new()
        .convert_bytes(b"{not json\0", StreamInfo::new().with_extension(".ipynb"))
        .unwrap();
    assert_eq!(raw.markdown, "{not json\0");
    assert!(raw.title.is_none());

    let atom = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Atom Title</title>
  <entry>
    <title>Atom Item</title>
    <published>2024-01-01T00:00:00Z</published>
    <updated>2024-02-02T00:00:00Z</updated>
    <link href="https://example.com/atom"/>
    <summary>Plain summary</summary>
  </entry>
</feed>"#;
    let result = MarkItDown::new()
        .convert_bytes(atom.as_bytes(), StreamInfo::new().with_extension(".atom"))
        .unwrap();
    assert!(result.markdown.contains("# Atom Title"));
    assert!(result.markdown.contains("## Atom Item"));
    assert!(result.markdown.contains("Published on:"));
    assert!(result.markdown.contains("Updated on:"));
    assert!(result.markdown.contains("https://example.com/atom"));
    assert!(result.markdown.contains("Plain summary"));

    let markdown = MarkItDown::new()
        .convert_bytes(
            b"# Title\n\nbody  \n",
            StreamInfo::new().with_extension(".md"),
        )
        .unwrap();
    assert_eq!(markdown.markdown, "# Title\n\nbody\n");
    let jsonl = b"{\"a\":1}\n{\"b\":2}\n";
    let result = MarkItDown::new()
        .convert_bytes(jsonl, StreamInfo::new().with_extension(".jsonl"))
        .unwrap();
    assert_eq!(result.markdown, "{\"a\":1}\n{\"b\":2}\n");
}

#[test]
fn empty_input_is_unsupported_and_readers_and_paths_work() {
    let err = MarkItDown::new()
        .convert_bytes(b"", StreamInfo::new())
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err}");

    let via_reader = MarkItDown::new()
        .convert_reader(
            Cursor::new(b"name,city\nAda,London\n"),
            StreamInfo::new().with_extension("csv"),
        )
        .unwrap();
    assert!(via_reader.markdown.contains("Ada"));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notes.csv");
    std::fs::write(&path, "h\n1\n").unwrap();
    let via_path = MarkItDown::new().convert_path(&path).unwrap();
    assert!(via_path.markdown.contains("| 1 |"));

    let missing = MarkItDown::new()
        .convert_path(dir.path().join("missing.csv"))
        .unwrap_err();
    assert!(matches!(missing, Error::Io(_)), "{missing}");
    assert_eq!(markitdown::version(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn data_uris_cover_base64_charset_and_rejection() {
    let engine = MarkItDown::new();
    let plain = engine
        .convert_uri("data:text/plain;base64,aGVs bG8=")
        .unwrap();
    assert_eq!(plain.markdown, "hello");

    let latin1 = engine
        .convert_uri("data:text/plain;charset=\"iso-8859-1\",caf%E9")
        .unwrap();
    assert_eq!(latin1.markdown, "caf\u{e9}");

    let csv = engine
        .convert_uri("data:text/csv;base64,bmFtZSxjaXR5CkFkYSxMb25kb24K")
        .unwrap();
    assert!(csv.markdown.contains("| Ada | London |"));

    for uri in [
        "data:text/plain",
        "data:text/plain;base64,!!!!",
        "ftp://example.com/a.txt",
        "not a uri",
    ] {
        let err = engine.convert_uri(uri).unwrap_err();
        assert!(matches!(err, Error::Fetch(_)), "{uri}: {err}");
    }
}

#[test]
fn zip_limits_directories_encryption_and_lying_sizes() {
    let archive = zip_stored(&[("notes.csv", b"name,city\nAda,London\n")]);
    let too_many = MarkItDown::with_options(Options {
        max_zip_entries: 0,
        ..Options::default()
    })
    .convert_bytes(&archive, StreamInfo::new().with_extension(".zip"))
    .unwrap_err();
    assert!(too_many.to_string().contains("entries"), "{too_many}");

    let too_big = MarkItDown::with_options(Options {
        max_zip_uncompressed_bytes: 4,
        ..Options::default()
    })
    .convert_bytes(&archive, StreamInfo::new().with_extension(".zip"))
    .unwrap_err();
    assert!(too_big.to_string().contains("uncompressed"), "{too_big}");

    let shallow = MarkItDown::with_options(Options {
        max_zip_depth: 0,
        ..Options::default()
    })
    .convert_bytes(&archive, StreamInfo::new().with_extension(".zip"))
    .unwrap_err();
    assert!(matches!(shallow, Error::FileConversion(_)), "{shallow}");
    assert!(shallow.to_string().contains("nesting"), "{shallow}");

    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.add_directory("subdir/", options).unwrap();
    writer.start_file("subdir/notes.csv", options).unwrap();
    writer.write_all(b"name,city\nAda,London\n").unwrap();
    writer.start_file("docs\\plain.csv", options).unwrap();
    writer.write_all(b"h\n1\n").unwrap();
    writer.start_file("/abs.csv", options).unwrap();
    writer.write_all(b"SECRETABS").unwrap();
    let encrypted = options.with_aes_encryption(zip::AesMode::Aes128, "secret");
    writer.start_file("secret.csv", encrypted).unwrap();
    writer.write_all(b"hidden\nSECRETENC\n").unwrap();
    let mixed = writer.finish().unwrap().into_inner();
    let result = MarkItDown::new()
        .convert_bytes(&mixed, StreamInfo::new().with_extension(".zip"))
        .unwrap();
    assert!(result.markdown.contains("## File: subdir/notes.csv"));
    assert!(result.markdown.contains("Ada"));
    assert!(result.markdown.contains("## File: docs/plain.csv"));
    assert!(!result.markdown.contains("## File: subdir/\n"));
    assert!(!result.markdown.contains("SECRETABS"));
    assert!(!result.markdown.contains("SECRETENC"));

    let lying = zip_with_lying_uncompressed_size("note.csv", b"name,city\nAda,London\n");
    let err = MarkItDown::new()
        .convert_bytes(&lying, StreamInfo::new().with_extension(".zip"))
        .unwrap_err();
    assert!(err.to_string().contains("declared size"), "{err}");
}

fn zip_with_lying_uncompressed_size(name: &str, data: &[u8]) -> Vec<u8> {
    let mut patched = zip_stored(&[(name, data)]);
    let needle = (data.len() as u32).to_le_bytes();
    let mut index = 0;
    let mut hits = 0;
    while index + 8 <= patched.len() {
        if patched[index..index + 4] == needle && patched[index + 4..index + 8] == needle {
            patched[index + 4..index + 8].copy_from_slice(&0u32.to_le_bytes());
            hits += 1;
            index += 8;
        } else {
            index += 1;
        }
    }
    assert!(
        hits >= 2,
        "expected local and central size fields, patched {hits}"
    );
    patched
}

#[test]
fn image_mime_bad_audio_and_bad_pdf() {
    let png = MarkItDown::new()
        .convert_bytes(PNG_1X1, StreamInfo::new().with_mimetype("image/png"))
        .unwrap();
    assert!(png.markdown.contains("Format: PNG"));
    assert!(png.markdown.contains("Dimensions: 1 x 1"));

    let err = MarkItDown::new()
        .convert_bytes(&[0, 1, 2, 3, 4], StreamInfo::new().with_extension(".wav"))
        .unwrap_err();
    assert!(failed_with(&err, "audio"), "{err}");

    let err = MarkItDown::new()
        .convert_bytes(
            b"%PDF-1.4\nnot a pdf",
            StreamInfo::new().with_extension(".pdf"),
        )
        .unwrap_err();
    assert!(failed_with(&err, "pdf"), "{err}");
}

fn failed_with(err: &Error, converter: &str) -> bool {
    match err {
        Error::FileConversion(attempts) => attempts
            .iter()
            .any(|attempt| attempt.converter == converter),
        _ => false,
    }
}

#[test]
fn cli_hints_validation_and_version() {
    let bin = env!("CARGO_BIN_EXE_markitdown");
    let stdin_ok = Command::new(bin)
        .args(["-x", "csv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = write_stdin(stdin_ok, b"a,b\n1,2\n", true);
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("| 1 | 2 |"));

    let mime_ok = Command::new(bin)
        .args(["-m", "text/csv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = write_stdin(mime_ok, b"a,b\n1,2\n", true);
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("| 1 | 2 |"));

    let latin1 = Command::new(bin)
        .args(["-x", "txt", "-c", "iso-8859-1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let output = write_stdin(latin1, &[b'c', b'a', b'f', 0xE9], true);
    assert!(String::from_utf8(output.stdout)
        .unwrap()
        .contains("caf\u{e9}"));

    for args in [
        vec!["-c", "not-a-charset"],
        vec!["-m", "csv"],
        vec!["-x", "."],
        vec!["-x", "../secret"],
        vec!["--use-content-understanding"],
    ] {
        let output = Command::new(bin)
            .args(&args)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "{args:?} {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let missing = Command::new(bin)
        .arg("/tmp/markitdown-rust-missing-file.csv")
        .output()
        .unwrap();
    assert!(!missing.status.success());

    let version = Command::new(bin).arg("-v").output().unwrap();
    assert!(version.status.success());
    let text = String::from_utf8_lossy(&version.stdout);
    assert!(text.contains(env!("CARGO_PKG_VERSION")), "{text}");

    let help = Command::new(bin).arg("--help").output().unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("extension"));
}

fn write_stdin(
    mut child: std::process::Child,
    bytes: &[u8],
    expect_ok: bool,
) -> std::process::Output {
    child.stdin.as_mut().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.success(), expect_ok, "{output:?}");
    output
}

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];
