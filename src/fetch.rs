#[cfg(feature = "url")]
use std::io::Read;
#[cfg(any(feature = "url", test))]
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};

use url::Url;

use crate::engine::MarkItDown;
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

#[cfg(feature = "url")]
const MAX_BODY: u64 = 64 * 1024 * 1024;
#[cfg(feature = "url")]
const MAX_REDIRECTS: usize = 5;

/// Fetch `uri` and convert it.
///
/// `file` and `data` URIs need no network. `http` and `https` require the `url`
/// feature. Each HTTP target is checked before the connection and again on
/// every redirect.
pub fn convert_uri(
    engine: &MarkItDown,
    uri: &str,
) -> Result<crate::convert::DocumentConverterResult> {
    if let Some(rest) = uri.strip_prefix("data:") {
        return convert_data(engine, uri, rest);
    }
    let parsed = Url::parse(uri).map_err(|err| Error::Fetch(err.to_string()))?;
    match parsed.scheme() {
        "file" => convert_file(engine, &parsed),
        "http" | "https" => convert_http(engine, &parsed),
        other => Err(Error::Fetch(format!("unsupported URI scheme {other}"))),
    }
}

fn convert_file(engine: &MarkItDown, url: &Url) -> Result<crate::convert::DocumentConverterResult> {
    let path = url
        .to_file_path()
        .map_err(|()| Error::Fetch(format!("file URI is not a local path: {url}")))?;
    engine.convert_path(path)
}

fn convert_data(
    engine: &MarkItDown,
    uri: &str,
    rest: &str,
) -> Result<crate::convert::DocumentConverterResult> {
    let (meta, payload) = rest
        .split_once(',')
        .ok_or_else(|| Error::Fetch("data URI is missing a comma".into()))?;
    let mut mime = "text/plain".to_string();
    let mut charset = None;
    let mut base64_data = false;
    if !meta.is_empty() {
        for (index, part) in meta.split(';').enumerate() {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if part.eq_ignore_ascii_case("base64") {
                base64_data = true;
            } else if let Some((key, value)) = part.split_once('=') {
                if key.trim().eq_ignore_ascii_case("charset") {
                    charset = Some(value.trim().trim_matches('"').to_string());
                }
            } else if index == 0 && part.contains('/') {
                mime = part.to_string();
            }
        }
    }
    let bytes = if base64_data {
        decode_base64(payload)?
    } else {
        percent_encoding::percent_decode_str(payload).collect()
    };
    let mut info = StreamInfo::new().with_mimetype(mime).with_url(uri);
    if let Some(charset) = charset {
        info = info.with_charset(charset);
    }
    engine.convert_bytes(&bytes, info)
}

fn decode_base64(payload: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let compact: String = payload.chars().filter(|ch| !ch.is_whitespace()).collect();
    if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(compact.as_bytes()) {
        return Ok(bytes);
    }
    base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(compact.as_bytes())
        .map_err(|_| Error::Fetch("data URI base64 payload is invalid".into()))
}

#[cfg(feature = "url")]
fn convert_http(engine: &MarkItDown, url: &Url) -> Result<crate::convert::DocumentConverterResult> {
    check_http_target(url).map_err(Error::Fetch)?;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                return attempt.error("too many redirects");
            }
            match check_http_target(attempt.url()) {
                Ok(()) => attempt.follow(),
                Err(message) => attempt.error(message),
            }
        }))
        .build()
        .map_err(|err| Error::Fetch(err.to_string()))?;
    let response = client
        .get(url.clone())
        .header(
            reqwest::header::ACCEPT,
            "text/markdown, text/html;q=0.9, text/plain;q=0.8, */*;q=0.1",
        )
        .send()
        .map_err(|err| Error::Fetch(err.to_string()))?;
    if !response.status().is_success() {
        return Err(Error::Fetch(format!("HTTP {}", response.status())));
    }
    let final_url = response.url().clone();
    check_http_target(&final_url).map_err(Error::Fetch)?;
    if let Some(length) = response.content_length() {
        if length > MAX_BODY {
            return Err(Error::Fetch(format!("response exceeds {MAX_BODY} bytes")));
        }
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let filename = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(filename_from_disposition);
    let mut body = Vec::new();
    response
        .take(MAX_BODY + 1)
        .read_to_end(&mut body)
        .map_err(|err| Error::Fetch(err.to_string()))?;
    if body.len() as u64 > MAX_BODY {
        return Err(Error::Fetch(format!("response exceeds {MAX_BODY} bytes")));
    }
    let (mime, charset) = content_type
        .as_deref()
        .map(split_content_type)
        .unwrap_or((None, None));
    let mut info = StreamInfo::new().with_url(final_url.as_str());
    if let Some(filename) = filename {
        if info.extension.is_none() {
            if let Some(extension) = std::path::Path::new(&filename).extension() {
                info = info.with_extension(extension.to_string_lossy().as_ref());
            }
        }
        info.filename = Some(filename);
    }
    if let Some(mime) = mime {
        info = info.with_mimetype(mime);
    }
    if let Some(charset) = charset {
        info = info.with_charset(charset);
    }
    engine.convert_bytes(&body, info)
}

#[cfg(not(feature = "url"))]
fn convert_http(
    _engine: &MarkItDown,
    url: &Url,
) -> Result<crate::convert::DocumentConverterResult> {
    let _ = url;
    Err(Error::Fetch(
        "fetching http(s) URLs requires the url feature".into(),
    ))
}

/// Reject non-HTTP schemes, metadata hostnames, and non-public addresses.
///
/// Domain names are resolved and every address is checked, including
/// IPv4-mapped IPv6. The check runs again inside the HTTP client, which
/// resolves the name on its own when it connects.
#[cfg(any(feature = "url", test))]
pub(crate) fn check_http_target(url: &Url) -> std::result::Result<(), String> {
    let scheme = url.scheme();
    if scheme != "http" && scheme != "https" {
        return Err(format!("refusing non-http(s) URL {url}"));
    }
    match url.host() {
        Some(url::Host::Ipv4(address)) => {
            if is_blocked_v4(address) {
                Err(format!("refusing address {address}"))
            } else {
                Ok(())
            }
        }
        Some(url::Host::Ipv6(address)) => {
            if is_blocked_ip(IpAddr::V6(address)) {
                Err(format!("refusing address {address}"))
            } else {
                Ok(())
            }
        }
        Some(url::Host::Domain(domain)) => check_domain(url, domain),
        None => Err(format!("URL has no host: {url}")),
    }
}

#[cfg(any(feature = "url", test))]
fn check_domain(url: &Url, domain: &str) -> std::result::Result<(), String> {
    let host = domain.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host == "metadata.google.internal"
    {
        return Err(format!("refusing host {host}"));
    }
    let port = url.port_or_known_default().unwrap_or(80);
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|err| format!("could not resolve {host}: {err}"))?;
    let mut saw_address = false;
    for address in addresses {
        saw_address = true;
        let ip = address.ip();
        if is_blocked_ip(ip) {
            return Err(format!("refusing address {ip}"));
        }
    }
    if !saw_address {
        return Err(format!("could not resolve {host}"));
    }
    Ok(())
}

#[cfg(any(feature = "url", test))]
fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(address) => is_blocked_v4(address),
        IpAddr::V6(address) => {
            if let Some(mapped) = address.to_ipv4_mapped() {
                return is_blocked_v4(mapped);
            }
            address.is_loopback()
                || address.is_unspecified()
                || address.is_unicast_link_local()
                || address.is_multicast()
                || is_ula(address)
        }
    }
}

#[cfg(any(feature = "url", test))]
fn is_blocked_v4(address: Ipv4Addr) -> bool {
    address.is_loopback()
        || address.is_private()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_broadcast()
        || address.is_multicast()
        || is_cgnat(address)
}

/// Shared address space, 100.64.0.0/10.
#[cfg(any(feature = "url", test))]
fn is_cgnat(address: Ipv4Addr) -> bool {
    let [first, second, _, _] = address.octets();
    first == 100 && (64..128).contains(&second)
}

/// Unique local addresses, fc00::/7.
#[cfg(any(feature = "url", test))]
fn is_ula(address: Ipv6Addr) -> bool {
    address.segments()[0] & 0xfe00 == 0xfc00
}

#[cfg(feature = "url")]
fn split_content_type(value: &str) -> (Option<String>, Option<String>) {
    let mut parts = value.split(';');
    let mime = parts.next().unwrap_or("").trim();
    let mime = if mime.is_empty() {
        None
    } else {
        Some(mime.to_string())
    };
    let mut charset = None;
    for part in parts {
        if let Some((key, value)) = part.split_once('=') {
            if key.trim().eq_ignore_ascii_case("charset") {
                let value = value.trim().trim_matches('"');
                if !value.is_empty() {
                    charset = Some(value.to_string());
                }
            }
        }
    }
    (mime, charset)
}

#[cfg(feature = "url")]
fn filename_from_disposition(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if let Some(index) = lower.find("filename*=") {
        let rest = value[index + "filename*=".len()..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');
        let encoded = rest.split("''").nth(1).unwrap_or(rest);
        let decoded = percent_encoding::percent_decode_str(encoded)
            .decode_utf8_lossy()
            .into_owned();
        if !decoded.is_empty() {
            return Some(decoded);
        }
    }
    if let Some(index) = lower.find("filename=") {
        let rest = value[index + "filename=".len()..]
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .trim_matches('"');
        if !rest.is_empty() && !rest.contains('*') {
            return Some(rest.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::check_http_target;
    use url::Url;

    fn assert_blocked(url: &str) {
        let parsed = Url::parse(url).unwrap();
        assert!(
            check_http_target(&parsed).is_err(),
            "{url} should be refused"
        );
    }

    #[test]
    fn rejects_private_and_metadata_targets() {
        for url in [
            "http://127.0.0.1/",
            "http://127.0.0.1:8080/latest/meta-data",
            "http://169.254.169.254/",
            "http://169.254.1.1/",
            "http://10.1.2.3/",
            "http://192.168.1.1/",
            "http://172.16.0.1/",
            "http://0.0.0.0/",
            "http://100.64.0.1/",
            "http://255.255.255.255/",
            "http://[::1]/",
            "http://[fe80::1]/",
            "http://[fc00::1]/",
            "http://[::ffff:127.0.0.1]/",
            "http://[::ffff:10.1.2.3]/",
            "http://[::ffff:169.254.169.254]/",
            "http://[::]/",
            "http://[fd12::1]/",
            "http://[ff02::1]/",
            "http://127.0.0.2/",
            "http://224.0.0.1/",
            "http://100.127.255.255/",
            "http://172.31.255.255/",
            "http://localhost/",
            "http://localhost./",
            "http://foo.localhost/",
            "http://metadata.google.internal/",
            "http://metadata.google.internal./computeMetadata/",
            "ftp://1.1.1.1/",
        ] {
            assert_blocked(url);
        }
    }

    #[test]
    fn allows_public_address_literals() {
        for url in [
            "http://1.1.1.1/",
            "http://8.8.8.8/",
            "https://100.128.0.1/",
            "https://100.63.255.255/",
            "http://172.15.255.255/",
            "http://172.32.0.1/",
            "https://[2606:4700:4700::1111]/",
        ] {
            let parsed = Url::parse(url).unwrap();
            assert!(
                check_http_target(&parsed).is_ok(),
                "{url} should be allowed: {:?}",
                check_http_target(&parsed)
            );
        }
    }

    #[cfg(feature = "url")]
    #[test]
    fn splits_content_type_and_reads_disposition_names() {
        assert_eq!(
            super::split_content_type("text/html; charset=\"utf-8\""),
            (Some("text/html".into()), Some("utf-8".into()))
        );
        assert_eq!(
            super::split_content_type("text/plain; charset="),
            (Some("text/plain".into()), None)
        );
        assert_eq!(
            super::filename_from_disposition(
                "attachment; filename=\"report.csv\"; filename*=UTF-8''notes%20%231.csv"
            )
            .as_deref(),
            Some("notes #1.csv")
        );
        assert_eq!(
            super::filename_from_disposition("inline; filename=plain.txt").as_deref(),
            Some("plain.txt")
        );
        assert_eq!(super::filename_from_disposition("inline"), None);
    }
}
