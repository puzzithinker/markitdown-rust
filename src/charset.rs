/// Decode bytes to a string.
///
/// A caller-supplied charset wins. Otherwise the encoding is guessed, and
/// UTF-8 lossy decoding is the last resort. A leading UTF-8 BOM is removed.
pub fn decode_text(bytes: &[u8], charset: Option<&str>) -> String {
    let bytes = strip_utf8_bom(bytes);
    if let Some(label) = charset {
        if let Some(encoding) = encoding_rs::Encoding::for_label(label.as_bytes()) {
            let (text, _, _) = encoding.decode(bytes);
            return text.into_owned();
        }
    }
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let (text, _, _) = detector.guess(None, true).decode(bytes);
    text.into_owned()
}

pub fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes)
}

#[cfg(test)]
mod tests {
    use super::decode_text;

    #[test]
    fn honors_a_known_charset_and_strips_a_utf8_bom() {
        let latin1 = [b'c', b'a', b'f', 0xE9];
        assert_eq!(decode_text(&latin1, Some("iso-8859-1")), "caf\u{e9}");
        assert_eq!(decode_text(&[0xEF, 0xBB, 0xBF, b'h', b'i'], None), "hi");
    }

    #[test]
    fn unknown_charset_falls_through_to_detection() {
        assert_eq!(
            decode_text("caf\u{e9}".as_bytes(), Some("not-a-charset")),
            "caf\u{e9}"
        );
    }
}
