use std::collections::HashMap;
use std::io::{Cursor, Read, Seek};

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::html::convert_html_str;
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

pub(crate) struct EpubConverter;

impl Converter for EpubConverter {
    fn name(&self) -> &'static str {
        "epub"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(info, &[".epub"]) || has_mime(info, &["application/epub+zip"])
    }

    fn convert(
        &self,
        bytes: &[u8],
        _info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|err| Error::Message(err.to_string()))?;
        let container = read_zip_string(&mut archive, "META-INF/container.xml")
            .ok_or_else(|| Error::Message("EPUB is missing META-INF/container.xml".into()))?;
        let opf_path = rootfile_path(&container)
            .ok_or_else(|| Error::Message("EPUB container has no rootfile".into()))?;
        let opf = read_zip_string(&mut archive, &opf_path)
            .ok_or_else(|| Error::Message(format!("EPUB is missing {opf_path}")))?;
        let package = parse_opf(&opf)?;
        let opf_dir = parent_dir(&opf_path);

        let mut markdown = String::new();
        if let Some(title) = package.title.as_ref().filter(|title| !title.is_empty()) {
            markdown.push_str(&format!("# {title}\n\n"));
        }
        if let Some(creator) = package
            .creator
            .as_ref()
            .filter(|creator| !creator.is_empty())
        {
            markdown.push_str(creator);
            markdown.push_str("\n\n");
        }

        for (index, idref) in package.spine.iter().enumerate() {
            let Some(href) = package.manifest.get(idref) else {
                continue;
            };
            let path = join_zip_path(&opf_dir, href);
            let Some(chapter) = read_zip_string(&mut archive, &path) else {
                continue;
            };
            let converted = convert_html_str(&chapter, ctx.options.keep_data_uris)?;
            let heading = converted
                .title
                .clone()
                .filter(|title| !title.is_empty())
                .unwrap_or_else(|| format!("Chapter {}", index + 1));
            if !converted.markdown.trim().is_empty() {
                markdown.push_str(&format!(
                    "## {heading}\n\n{}\n\n",
                    converted.markdown.trim()
                ));
            }
        }

        let mut result = DocumentConverterResult::new(markdown.trim_end());
        result.title = package.title.filter(|title| !title.is_empty());
        Ok(result)
    }
}

struct Opf {
    title: Option<String>,
    creator: Option<String>,
    manifest: HashMap<String, String>,
    spine: Vec<String>,
}

fn parse_opf(xml: &str) -> Result<Opf> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_metadata = false;
    let mut capture = Capture::None;
    let mut title = String::new();
    let mut creator = String::new();
    let mut manifest = HashMap::new();
    let mut spine = Vec::new();
    let mut current_id = None;
    let mut current_href = None;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event)) | Ok(Event::Empty(event)) => {
                let local = local_name(event.name().as_ref());
                match local.as_str() {
                    "metadata" => in_metadata = true,
                    "title" if in_metadata => capture = Capture::Title,
                    "creator" if in_metadata => capture = Capture::Creator,
                    "item" => {
                        current_id = attr_value(&event, b"id");
                        current_href = attr_value(&event, b"href");
                    }
                    "itemref" => {
                        if let Some(idref) = attr_value(&event, b"idref") {
                            spine.push(idref);
                        }
                    }
                    _ => {}
                }
                if local == "item" {
                    if let (Some(id), Some(href)) = (current_id.take(), current_href.take()) {
                        manifest.insert(id, href);
                    }
                }
            }
            Ok(Event::End(event)) => {
                let local = local_name(event.name().as_ref());
                match local.as_str() {
                    "metadata" => in_metadata = false,
                    "title" | "creator" => capture = Capture::None,
                    _ => {}
                }
            }
            Ok(Event::Text(event)) => {
                let text = event
                    .decode()
                    .map_err(|err| Error::Message(err.to_string()))?
                    .into_owned();
                match capture {
                    Capture::Title => title.push_str(&text),
                    Capture::Creator => creator.push_str(&text),
                    Capture::None => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(err) => return Err(Error::Message(err.to_string())),
            _ => {}
        }
        buf.clear();
    }

    Ok(Opf {
        title: nonempty(title),
        creator: nonempty(creator),
        manifest,
        spine,
    })
}

#[derive(Clone, Copy)]
enum Capture {
    None,
    Title,
    Creator,
}

fn attr_value(event: &quick_xml::events::BytesStart<'_>, name: &[u8]) -> Option<String> {
    event
        .attributes()
        .filter_map(|attr| attr.ok())
        .find_map(|attr| {
            if attr.key.as_ref() == name || attr.key.local_name().as_ref() == name {
                attr.unescape_value().ok().map(|value| value.into_owned())
            } else {
                None
            }
        })
}

fn local_name(qname: &[u8]) -> String {
    let name = qname.rsplit(|byte| *byte == b':').next().unwrap_or(qname);
    String::from_utf8_lossy(name).to_ascii_lowercase()
}

fn nonempty(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn rootfile_path(container_xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(container_xml);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(event) | Event::Empty(event)) => {
                if local_name(event.name().as_ref()) == "rootfile" {
                    if let Some(path) = attr_value(&event, b"full-path") {
                        return Some(path);
                    }
                }
            }
            Ok(Event::Eof) => return None,
            Err(_) => return None,
            _ => {}
        }
        buf.clear();
    }
}

fn parent_dir(path: &str) -> String {
    match path.rfind('/') {
        Some(index) => path[..index].to_string(),
        None => String::new(),
    }
}

fn join_zip_path(base_dir: &str, href: &str) -> String {
    let href = href.split('#').next().unwrap_or(href);
    let href = percent_encoding::percent_decode_str(href).decode_utf8_lossy();
    let href = href.replace('\\', "/");
    if href.starts_with('/') {
        return href.trim_start_matches('/').to_string();
    }
    let mut parts: Vec<String> = base_dir
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other.to_string()),
        }
    }
    parts.join("/")
}

fn read_zip_string<R: Read + Seek>(archive: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let index = (0..archive.len()).find(|&index| {
        archive
            .by_index(index)
            .ok()
            .is_some_and(|file| zip_name_eq(file.name(), name))
    })?;
    let mut file = archive.by_index(index).ok()?;
    let mut buf = String::new();
    file.read_to_string(&mut buf).ok()?;
    Some(buf)
}

fn zip_name_eq(candidate: &str, name: &str) -> bool {
    let candidate = candidate.replace('\\', "/");
    let candidate = candidate.trim_start_matches('/');
    candidate == name || candidate.eq_ignore_ascii_case(name)
}
