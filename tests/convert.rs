use std::io::{Cursor, Write};
use std::process::{Command, Stdio};

use markitdown::{Error, MarkItDown, Options, StreamInfo};

fn convert(bytes: &[u8], info: StreamInfo) -> markitdown::DocumentConverterResult {
    MarkItDown::new()
        .convert_bytes(bytes, info)
        .unwrap_or_else(|err| panic!("convert failed: {err}"))
}

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

#[test]
fn csv_quoting_pipes_short_rows_bom_and_blank_rows() {
    let csv = "\u{feff}\nname,note\n\n\"Ada, Lovelace\",\"a|b\"\nshort\n\n";
    let result = convert(csv.as_bytes(), StreamInfo::new().with_extension(".csv"));
    assert_eq!(
        result.markdown,
        "| name | note |\n| --- | --- |\n| Ada, Lovelace | a\\|b |\n| short |  |"
    );
}

#[test]
fn csv_escapes_a_backslash_before_a_pipe() {
    // Two backslashes before a pipe become four backslashes plus \|.
    let result = convert(b"h\na\\\\|b\n", StreamInfo::new().with_extension("csv"));
    assert_eq!(result.markdown, "| h |\n| --- |\n| a\\\\\\\\\\|b |");
}

#[test]
fn html_title_strips_script_and_style_and_truncates_data_uris() {
    let payload = "A".repeat(80);
    let html = format!(
        "<html><head><title>Page Title</title></head><body>\
         <script>SECRET_SCRIPT</script><style>SECRET_STYLE</style>\
         <p>Visible text</p>\
         <a href=\"data:image/png;base64,{payload}\">picture</a>\
         </body></html>"
    );
    let result = convert(html.as_bytes(), StreamInfo::new().with_extension(".html"));
    assert_eq!(result.title.as_deref(), Some("Page Title"));
    assert!(result.markdown.contains("Visible text"));
    assert!(!result.markdown.contains("SECRET_SCRIPT"));
    assert!(!result.markdown.contains("SECRET_STYLE"));
    assert!(result.markdown.contains("data:..."));
    assert!(!result.markdown.contains(&payload));

    let kept = MarkItDown::with_options(Options {
        keep_data_uris: true,
        ..Options::default()
    })
    .convert_bytes(html.as_bytes(), StreamInfo::new().with_extension(".html"))
    .unwrap();
    assert!(kept.markdown.contains(&payload));
}

#[test]
fn plain_text_and_json_pass_through_with_charset_hint() {
    let json = br#"{"b":1,"a":2}"#;
    let result = convert(json, StreamInfo::new().with_extension(".json"));
    assert_eq!(result.markdown, r#"{"b":1,"a":2}"#);

    let latin1 = [b'c', b'a', b'f', 0xE9];
    let result = convert(
        &latin1,
        StreamInfo::new()
            .with_extension(".txt")
            .with_charset("iso-8859-1"),
    );
    assert_eq!(result.markdown, "caf\u{e9}");
}

#[test]
fn ipynb_cells_and_title() {
    let notebook = r##"{
      "nbformat": 4,
      "metadata": {"title": "Demo", "language_info": {"name": "python"}},
      "cells": [
        {"cell_type": "markdown", "source": ["# Heading\n", "Intro"]},
        {"cell_type": "code", "source": "print(1)\n", "outputs": []},
        {"cell_type": "raw", "source": ["raw text"]}
      ]
    }"##;
    let result = convert(
        notebook.as_bytes(),
        StreamInfo::new().with_extension(".ipynb"),
    );
    assert_eq!(result.title.as_deref(), Some("Demo"));
    assert!(result.markdown.contains("# Heading"));
    assert!(result.markdown.contains("```python\nprint(1)\n\n```"));
    assert!(result.markdown.contains("```\nraw text\n```"));
}

#[test]
fn rss_items_and_non_feed_xml() {
    let feed = r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
  <title>Feed Title</title>
  <description>Feed desc</description>
  <item>
    <title>Item One</title>
    <link>https://example.com/one</link>
    <pubDate>Mon, 01 Jan 2024 00:00:00 GMT</pubDate>
    <description><![CDATA[<p>Hello <b>item</b></p>]]></description>
  </item>
</channel></rss>"#;
    let result = convert(feed.as_bytes(), StreamInfo::new().with_extension(".xml"));
    assert_eq!(result.title.as_deref(), Some("Feed Title"));
    assert!(result.markdown.contains("# Feed Title"));
    assert!(result.markdown.contains("## Item One"));
    assert!(result.markdown.contains("Published on:"));
    assert!(result.markdown.contains("https://example.com/one"));
    assert!(result.markdown.contains("Hello"));
    assert!(result.markdown.contains("item"));

    let xml = b"<note><to>Ada</to></note>";
    let result = convert(xml, StreamInfo::new().with_extension(".xml"));
    assert!(result.markdown.contains("<note>"));
    assert!(result.markdown.contains("Ada"));
}

#[test]
fn zip_converts_csv_skips_traversal_and_honors_depth() {
    let archive = zip_stored(&[
        ("notes.csv", b"name,city\nAda,London\n"),
        ("../secret.csv", b"SECRETDATA"),
        ("nested/../../also-secret.csv", b"ALSOSECRET"),
    ]);
    let result = convert(
        &archive,
        StreamInfo::new()
            .with_extension(".zip")
            .with_url("bundle.zip"),
    );
    assert!(result
        .markdown
        .contains("Content from the zip file `bundle.zip`:"));
    assert!(result.markdown.contains("## File: notes.csv"));
    assert!(result.markdown.contains("Ada"));
    assert!(!result.markdown.contains("SECRETDATA"));
    assert!(!result.markdown.contains("ALSOSECRET"));

    let inner = zip_stored(&[("hidden.csv", b"col\nHIDDEN\n")]);
    let outer = zip_stored(&[("visible.csv", b"col\nVISIBLE\n"), ("inner.zip", &inner)]);
    let limited = MarkItDown::with_options(Options {
        max_zip_depth: 1,
        ..Options::default()
    })
    .convert_bytes(&outer, StreamInfo::new().with_extension(".zip"))
    .unwrap();
    assert!(limited.markdown.contains("VISIBLE"));
    assert!(!limited.markdown.contains("HIDDEN"));
}

#[test]
fn minimal_pdf_yields_text() {
    let pdf = one_page_pdf(b"BT /F1 24 Tf 72 100 Td (Hello PDF) Tj ET");
    let result = convert(&pdf, StreamInfo::new().with_extension(".pdf"));
    assert!(
        result.markdown.contains("Hello"),
        "markdown was {:?}",
        result.markdown
    );
}

#[test]
fn minimal_docx_yields_paragraph() {
    let docx = minimal_docx("Hello DOCX");
    match MarkItDown::new().convert_bytes(&docx, StreamInfo::new().with_extension(".docx")) {
        Ok(result) => assert!(
            result.markdown.contains("Hello DOCX"),
            "markdown was {:?}",
            result.markdown
        ),
        Err(Error::UnsupportedFormat(message)) => {
            panic!("docx was treated as unsupported: {message}")
        }
        Err(Error::FileConversion(attempts)) => {
            panic!("undoc rejected the minimal docx: {attempts:?}")
        }
        Err(err) => panic!("{err}"),
    }

    let sniffed = MarkItDown::new()
        .convert_bytes(&docx, StreamInfo::new().with_extension(".zip"))
        .unwrap();
    assert!(sniffed.markdown.contains("Hello DOCX"));
    assert!(!sniffed.markdown.contains("Content from the zip file"));
}

#[test]
fn epub_chapter_beats_generic_zip() {
    let epub = minimal_epub();
    let result = convert(&epub, StreamInfo::new().with_extension(".zip"));
    assert!(
        result.markdown.contains("Hello EPUB"),
        "markdown was {:?}",
        result.markdown
    );
    assert!(result.markdown.contains("Sample Book"));
    assert!(!result.markdown.contains("Content from the zip file"));
}

#[test]
fn wikipedia_keeps_article_and_drops_chrome() {
    let html = r#"<html><head><title>Rust</title></head><body>
      <div id="mw-content-text"><div class="mw-parser-output">
        <p>Rust is a language.</p>
        <div class="navbox">Navigation box secret</div>
        <div class="reflist">Reference list secret</div>
      </div></div>
    </body></html>"#;
    let page = convert(
        html.as_bytes(),
        StreamInfo::new()
            .with_extension(".html")
            .with_url("https://en.wikipedia.org/wiki/Rust_(programming_language)"),
    );
    assert!(page.markdown.contains("Rust is a language"));
    assert!(!page.markdown.contains("Navigation box secret"));
    assert!(!page.markdown.contains("Reference list secret"));

    let saved = convert(html.as_bytes(), StreamInfo::new().with_extension(".html"));
    assert!(saved.markdown.contains("Navigation box secret"));
}

#[test]
fn data_and_file_uris() {
    let csv = MarkItDown::new()
        .convert_uri("data:text/csv,name%2Ccity%0AAda%2CLondon")
        .unwrap();
    assert!(csv.markdown.contains("| Ada | London |"));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.txt");
    std::fs::write(&path, "hello file\n").unwrap();
    let uri = format!("file://{}", path.display());
    let result = MarkItDown::new().convert_uri(&uri).unwrap();
    assert!(result.markdown.contains("hello file"));
}

#[test]
fn image_lists_format_and_size() {
    let result = convert(PNG_1X1, StreamInfo::new().with_extension(".png"));
    assert!(
        result.markdown.contains("Format: PNG"),
        "{}",
        result.markdown
    );
    assert!(
        result.markdown.contains("Dimensions: 1 x 1"),
        "{}",
        result.markdown
    );
}

#[test]
fn audio_lists_container() {
    let result = convert(&minimal_wav(), StreamInfo::new().with_extension(".wav"));
    assert!(
        result.markdown.contains("Container: WAV"),
        "{}",
        result.markdown
    );
    assert!(result.markdown.contains("Duration:"), "{}", result.markdown);
}

#[test]
fn unrecognized_bytes_are_unsupported_and_bad_msg_is_attempted() {
    let err = MarkItDown::new()
        .convert_bytes(&[0, 1, 2, 3, 4], StreamInfo::new())
        .unwrap_err();
    assert!(matches!(err, Error::UnsupportedFormat(_)), "{err}");

    let mut ole = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    ole.extend_from_slice(&[0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
    let err = MarkItDown::new()
        .convert_bytes(&ole, StreamInfo::new().with_extension(".msg"))
        .unwrap_err();
    assert!(matches!(err, Error::FileConversion(_)), "{err}");
}

#[test]
fn cli_output_file_stdin_and_azure_flag() {
    let bin = env!("CARGO_BIN_EXE_markitdown");
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.csv");
    let output = dir.path().join("out.md");
    std::fs::write(&input, "a,b\n1,2\n").unwrap();
    let status = Command::new(bin)
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let written = std::fs::read_to_string(&output).unwrap();
    assert!(written.contains("| a | b |"));
    assert!(!written.ends_with('\n'));

    let mut child = Command::new(bin)
        .arg("-x")
        .arg(".csv")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"a,b\n1,2\n")
        .unwrap();
    let collected = child.wait_with_output().unwrap();
    assert!(
        collected.status.success(),
        "{}",
        String::from_utf8_lossy(&collected.stderr)
    );
    let stdout = String::from_utf8(collected.stdout).unwrap();
    assert!(stdout.contains("| a | b |"));
    assert!(stdout.ends_with('\n'));

    let azure = Command::new(bin).arg("--use-docintel").output().unwrap();
    assert!(!azure.status.success());
    let stderr = String::from_utf8_lossy(&azure.stderr);
    assert!(stderr.contains("not included"), "{stderr}");

    let plugins = Command::new(bin).arg("--list-plugins").output().unwrap();
    assert!(plugins.status.success());
    let message = String::from_utf8_lossy(&plugins.stdout);
    assert!(message.contains("plugin"), "{message}");
}

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00,
    0x00, 0x00, 0x02, 0x00, 0x01, 0xE2, 0x21, 0xBC, 0x33, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
    0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn minimal_wav() -> Vec<u8> {
    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&8000u32.to_le_bytes());
    body.extend_from_slice(&8000u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&8u16.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&1u32.to_le_bytes());
    body.push(128);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend(body);
    bytes
}

fn one_page_pdf(content: &[u8]) -> Vec<u8> {
    let stream = {
        let mut out = format!("<</Length {}>>\nstream\n", content.len()).into_bytes();
        out.extend_from_slice(content);
        out.extend_from_slice(b"\nendstream");
        out
    };
    let objects = [
        b"<</Type/Catalog/Pages 2 0 R>>".to_vec(),
        b"<</Type/Pages/Kids[3 0 R]/Count 1>>".to_vec(),
        b"<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>".to_vec(),
        stream,
        b"<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_vec(),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<</Size {size}/Root 1 0 R>>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn minimal_docx(text: &str) -> Vec<u8> {
    let document = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body><w:p><w:r><w:t>{text}</w:t></w:r></w:p></w:body>
</w:document>"#
    );
    let content_types = br#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="xml" ContentType="application/xml"/>
  <Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
</Types>"#;
    let rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>
</Relationships>"#;
    let document_rels = br#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
</Relationships>"#;
    zip_stored(&[
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", rels),
        ("word/_rels/document.xml.rels", document_rels),
        ("word/document.xml", document.as_bytes()),
    ])
}

fn minimal_epub() -> Vec<u8> {
    let container = br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#;
    let opf = br#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Sample Book</dc:title>
    <dc:creator>Ada</dc:creator>
  </metadata>
  <manifest>
    <item id="c1" href="chapter.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine>
    <itemref idref="c1"/>
  </spine>
</package>"#;
    let chapter = br#"<?xml version="1.0"?>
<html xmlns="http://www.w3.org/1999/xhtml">
  <head><title>Chapter One</title></head>
  <body><p>Hello EPUB</p></body>
</html>"#;
    zip_stored(&[
        ("mimetype", b"application/epub+zip"),
        ("META-INF/container.xml", container),
        ("OEBPS/content.opf", opf),
        ("OEBPS/chapter.xhtml", chapter),
    ])
}
