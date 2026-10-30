use std::io::{Cursor, Read};

use crate::stream_info::{normalize_extension, some_extension, StreamInfo};

/// A type guessed from the leading bytes of an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sniffed {
    pub extension: String,
    pub mimetype: String,
    /// Specific formats outrank a missing or generic (`.zip`, `.bin`) hint.
    pub specific: bool,
}

const CFB_MAGIC: &[u8] = b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

/// Guess extension and MIME type from content.
pub fn sniff(bytes: &[u8]) -> Option<Sniffed> {
    if bytes.starts_with(b"%PDF-") || bytes.starts_with(b"%PDF") {
        return Some(sniffed(".pdf", "application/pdf", true));
    }
    if bytes.starts_with(CFB_MAGIC) {
        return sniff_cfb(bytes);
    }
    if is_zip_magic(bytes) {
        return sniff_zip(bytes);
    }
    if let Some(markup) = sniff_markup(bytes) {
        return Some(markup);
    }
    if let Some(kind) = infer::get(bytes) {
        if let Some(sniffed) = from_infer(kind.mime_type(), kind.extension()) {
            return Some(sniffed);
        }
    }
    if looks_like_text(bytes) {
        return Some(sniffed(".txt", "text/plain", false));
    }
    None
}

/// Build the stream-info guesses converters will be tried against.
///
/// A specific content type (OOXML, EPUB, PDF, HTML, and so on) is tried before
/// a missing or generic extension such as `.zip`. An already specific extension
/// stays first, and the sniff result is added after it when the two differ.
pub fn build_guesses(bytes: &[u8], mut base: StreamInfo) -> Vec<StreamInfo> {
    if let Some(extension) = base.extension.clone() {
        base.extension = some_extension(&extension);
    }
    if base.mimetype.is_none() {
        if let Some(extension) = base.extension.as_deref() {
            base.mimetype = mime_for_extension(extension).map(str::to_string);
        }
    }
    if base.extension.is_none() {
        if let Some(mimetype) = base.mimetype.as_deref() {
            base.extension = extension_for_mime(mimetype).map(str::to_string);
        }
    }

    let sniffed = sniff(bytes);
    let mut guesses = Vec::new();
    let generic = base
        .extension
        .as_deref()
        .map(is_generic_extension)
        .unwrap_or(true);

    if generic {
        if let Some(sniffed) = sniffed.clone() {
            if sniffed.specific {
                guesses.push(apply_sniff(base.clone(), &sniffed));
            }
        }
    }

    guesses.push(base.clone());
    if let Some(sniffed) = sniffed {
        let extra = apply_sniff(base, &sniffed);
        if !guesses.iter().any(|guess| same_type(guess, &extra)) {
            guesses.push(extra);
        }
    }
    guesses
}

/// Canonical extension for each media type. Extra spellings live in the alias tables.
const CANONICAL_TYPES: &[(&str, &str)] = &[
    (".pdf", "application/pdf"),
    (
        ".docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (
        ".xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    (
        ".pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    (".doc", "application/msword"),
    (".xls", "application/vnd.ms-excel"),
    (".ppt", "application/vnd.ms-powerpoint"),
    (".csv", "text/csv"),
    (".html", "text/html"),
    (".xhtml", "application/xhtml+xml"),
    (".json", "application/json"),
    (".md", "text/markdown"),
    (".txt", "text/plain"),
    (".xml", "application/xml"),
    (".rss", "application/rss+xml"),
    (".atom", "application/atom+xml"),
    (".epub", "application/epub+zip"),
    (".zip", "application/zip"),
    (".msg", "application/vnd.ms-outlook"),
    (".jpg", "image/jpeg"),
    (".png", "image/png"),
    (".gif", "image/gif"),
    (".webp", "image/webp"),
    (".bmp", "image/bmp"),
    (".tiff", "image/tiff"),
    (".mp3", "audio/mpeg"),
    (".wav", "audio/wav"),
    (".flac", "audio/flac"),
    (".m4a", "audio/mp4"),
];

const EXTRA_EXTENSIONS: &[(&str, &str)] = &[
    (".htm", "text/html"),
    (".jsonl", "application/json"),
    (".ipynb", "application/json"),
    (".markdown", "text/markdown"),
    (".text", "text/plain"),
    (".jpeg", "image/jpeg"),
    (".tif", "image/tiff"),
];

const EXTRA_MIMES: &[(&str, &str)] = &[
    ("application/csv", ".csv"),
    ("text/x-markdown", ".md"),
    ("text/xml", ".xml"),
    ("application/rss", ".rss"),
    ("application/atom", ".atom"),
    ("audio/x-wav", ".wav"),
    ("audio/x-flac", ".flac"),
];

pub fn mime_for_extension(extension: &str) -> Option<&'static str> {
    CANONICAL_TYPES
        .iter()
        .chain(EXTRA_EXTENSIONS.iter())
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(extension))
        .map(|(_, mime)| *mime)
}

pub fn extension_for_mime(mimetype: &str) -> Option<&'static str> {
    let mime = mimetype.split(';').next().unwrap_or(mimetype).trim();
    CANONICAL_TYPES
        .iter()
        .find(|(_, candidate)| candidate.eq_ignore_ascii_case(mime))
        .map(|(extension, _)| *extension)
        .or_else(|| {
            EXTRA_MIMES
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(mime))
                .map(|(_, extension)| *extension)
        })
}

fn sniffed(extension: &str, mimetype: &str, specific: bool) -> Sniffed {
    Sniffed {
        extension: extension.to_string(),
        mimetype: mimetype.to_string(),
        specific,
    }
}

fn apply_sniff(mut info: StreamInfo, sniffed: &Sniffed) -> StreamInfo {
    info.extension = Some(sniffed.extension.clone());
    info.mimetype = Some(sniffed.mimetype.clone());
    info
}

fn same_type(left: &StreamInfo, right: &StreamInfo) -> bool {
    left.extension == right.extension && left.mimetype == right.mimetype
}

fn is_generic_extension(extension: &str) -> bool {
    matches!(extension, ".zip" | ".bin" | ".dat")
}

fn is_zip_magic(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(b"PK\x05\x06")
        || bytes.starts_with(b"PK\x07\x08")
}

fn sniff_cfb(bytes: &[u8]) -> Option<Sniffed> {
    let _ = bytes;
    #[cfg(feature = "office")]
    if let Ok(format) = undoc::detect_format_from_bytes(bytes) {
        return Some(office_sniff(format));
    }
    #[cfg(feature = "outlook")]
    {
        Some(sniffed(".msg", "application/vnd.ms-outlook", true))
    }
    #[cfg(not(feature = "outlook"))]
    {
        let _ = bytes;
        None
    }
}

fn sniff_zip(bytes: &[u8]) -> Option<Sniffed> {
    if bytes
        .windows(20)
        .take(64)
        .any(|window| window == b"application/epub+zip")
    {
        return Some(sniffed(".epub", "application/epub+zip", true));
    }
    #[cfg(feature = "office")]
    if let Ok(format) = undoc::detect_format_from_bytes(bytes) {
        return Some(office_sniff(format));
    }
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).ok()?;
    let mut names = Vec::new();
    for index in 0..archive.len() {
        if let Ok(file) = archive.by_index(index) {
            names.push(file.name().replace('\\', "/"));
        }
    }
    if names.iter().any(|name| name == "word/document.xml") {
        return Some(sniffed(
            ".docx",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            true,
        ));
    }
    if names.iter().any(|name| name == "xl/workbook.xml") {
        return Some(sniffed(
            ".xlsx",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            true,
        ));
    }
    if names.iter().any(|name| name == "ppt/presentation.xml") {
        return Some(sniffed(
            ".pptx",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            true,
        ));
    }
    if let Some(mimetype) = zip_entry_text(&mut archive, "mimetype") {
        if mimetype.trim() == "application/epub+zip" {
            return Some(sniffed(".epub", "application/epub+zip", true));
        }
    }
    Some(sniffed(".zip", "application/zip", false))
}

fn zip_entry_text<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut file = archive.by_name(name).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    Some(buf)
}

#[cfg(feature = "office")]
fn office_sniff(format: undoc::FormatType) -> Sniffed {
    let extension = format!(".{}", format.extension());
    let mimetype = mime_for_extension(&extension)
        .unwrap_or("application/octet-stream")
        .to_string();
    Sniffed {
        extension,
        mimetype,
        specific: true,
    }
}

fn sniff_markup(bytes: &[u8]) -> Option<Sniffed> {
    let sample = &bytes[..bytes.len().min(4096)];
    let text = String::from_utf8_lossy(sample);
    let trimmed = text.trim_start_matches('\u{feff}').trim_start();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || lower.starts_with("<head")
        || lower.starts_with("<body")
    {
        return Some(sniffed(".html", "text/html", true));
    }
    let atom = lower.starts_with("<feed")
        || (lower.contains("<feed") && lower.contains("<entry") && !lower.contains("<rss"));
    if atom {
        return Some(sniffed(".atom", "application/atom+xml", true));
    }
    if lower.starts_with("<rss") || (lower.contains("<rss") && lower.contains("<channel")) {
        return Some(sniffed(".rss", "application/rss+xml", true));
    }
    if lower.starts_with("<?xml") || (lower.starts_with("<") && lower.contains("xmlns")) {
        return Some(sniffed(".xml", "application/xml", true));
    }
    None
}

fn from_infer(mime: &str, extension: &str) -> Option<Sniffed> {
    let mime_l = mime.to_ascii_lowercase();
    if mime_l == "application/zip" || mime_l == "application/octet-stream" || extension.is_empty() {
        return None;
    }
    let extension = normalize_extension(extension);
    let specific = !is_generic_extension(&extension) && mime_l != "text/plain";
    Some(Sniffed {
        extension,
        mimetype: mime_l,
        specific,
    })
}

fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let sample = &bytes[..bytes.len().min(8192)];
    if sample.contains(&0) {
        return false;
    }
    let printable = sample
        .iter()
        .filter(|byte| byte.is_ascii_graphic() || byte.is_ascii_whitespace() || **byte >= 0x80)
        .count();
    printable * 10 >= sample.len() * 9
}

#[cfg(test)]
mod tests {
    use super::{build_guesses, sniff};
    use crate::stream_info::StreamInfo;
    use std::io::{Cursor, Write};

    fn zip_stored(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in files {
            writer.start_file(*name, options).unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn docx_bytes_are_not_a_generic_zip() {
        let bytes = zip_stored(&[("word/document.xml", b"<w:document/>")]);
        let sniffed = sniff(&bytes).expect("sniff");
        assert_eq!(sniffed.extension, ".docx");
        assert!(sniffed.specific);
    }

    #[test]
    fn generic_zip_extension_yields_to_docx_sniff() {
        let bytes = zip_stored(&[("word/document.xml", b"<w:document/>")]);
        let guesses = build_guesses(&bytes, StreamInfo::new().with_extension(".zip"));
        assert_eq!(guesses[0].extension.as_deref(), Some(".docx"));
    }

    #[test]
    fn epub_mimetype_wins_over_zip() {
        let bytes = zip_stored(&[("mimetype", b"application/epub+zip")]);
        let sniffed = sniff(&bytes).expect("sniff");
        assert_eq!(sniffed.extension, ".epub");
        assert!(sniffed.specific);
        let guesses = build_guesses(&bytes, StreamInfo::new().with_extension(".zip"));
        assert_eq!(guesses[0].extension.as_deref(), Some(".epub"));
    }

    #[test]
    fn plain_zip_stays_generic() {
        let bytes = zip_stored(&[("hello.txt", b"hi")]);
        let sniffed = sniff(&bytes).expect("sniff");
        assert_eq!(sniffed.extension, ".zip");
        assert!(!sniffed.specific);
    }

    #[test]
    fn atom_feed_is_not_rss() {
        let bytes = b"<?xml version=\"1.0\"?><feed xmlns=\"http://www.w3.org/2005/Atom\"><entry><title>A</title></entry></feed>";
        let sniffed = sniff(bytes).expect("sniff");
        assert_eq!(sniffed.extension, ".atom");
    }

    #[test]
    fn canonical_types_round_trip_and_aliases_collapse() {
        for (extension, mime) in super::CANONICAL_TYPES {
            assert_eq!(super::mime_for_extension(extension), Some(*mime));
            assert_eq!(super::extension_for_mime(mime), Some(*extension));
        }
        assert_eq!(super::mime_for_extension(".HTM"), Some("text/html"));
        assert_eq!(super::mime_for_extension(".jpeg"), Some("image/jpeg"));
        assert_eq!(
            super::mime_for_extension(".ipynb"),
            Some("application/json")
        );
        assert_eq!(
            super::extension_for_mime("TEXT/HTML; charset=UTF-8"),
            Some(".html")
        );
        assert_eq!(super::extension_for_mime("application/csv"), Some(".csv"));
        assert_eq!(super::extension_for_mime("text/xml"), Some(".xml"));
        assert_eq!(super::extension_for_mime("audio/x-wav"), Some(".wav"));
        assert_eq!(super::extension_for_mime("application/not-a-type"), None);
        assert_eq!(super::mime_for_extension(".unknown"), None);
    }

    #[test]
    fn specific_extension_stays_ahead_of_a_text_sniff() {
        let guesses = build_guesses(
            b"<html><body>hi</body></html>",
            StreamInfo::new().with_extension("csv"),
        );
        assert_eq!(guesses[0].extension.as_deref(), Some(".csv"));
        assert_eq!(guesses[0].mimetype.as_deref(), Some("text/csv"));
        assert!(guesses
            .iter()
            .any(|guess| guess.extension.as_deref() == Some(".html")));
    }

    #[test]
    fn generic_bin_yields_to_html_and_pdf_magic_is_specific() {
        let html = b"<!DOCTYPE html><html><title>T</title></html>";
        let guesses = build_guesses(html, StreamInfo::new().with_extension(".bin"));
        assert_eq!(guesses[0].extension.as_deref(), Some(".html"));

        let pdf = sniff(b"%PDF-1.7 garbage").expect("pdf");
        assert_eq!(pdf.extension, ".pdf");
        assert!(pdf.specific);
        let guesses = build_guesses(b"%PDF-1.7 garbage", StreamInfo::new());
        assert_eq!(guesses[0].extension.as_deref(), Some(".pdf"));
    }

    #[test]
    fn office_entry_names_and_rss_markup_are_specific() {
        let xlsx = sniff(&zip_stored(&[("xl/workbook.xml", b"<workbook/>")])).expect("xlsx");
        assert_eq!(xlsx.extension, ".xlsx");
        let pptx =
            sniff(&zip_stored(&[("ppt/presentation.xml", b"<presentation/>")])).expect("pptx");
        assert_eq!(pptx.extension, ".pptx");
        let rss =
            sniff(b"<rss version=\"2.0\"><channel><title>T</title></channel></rss>").expect("rss");
        assert_eq!(rss.extension, ".rss");
    }

    #[test]
    fn empty_and_dot_suffixes_fall_through_to_sniffing() {
        let guesses = build_guesses(b"%PDF-1.4\n", StreamInfo::new().with_extension("."));
        assert_eq!(guesses[0].extension.as_deref(), Some(".pdf"));
        assert!(sniff(&[]).is_none());
        assert!(sniff(&[0, 1, 2, 3]).is_none());
    }
}
