mod csv;
mod ipynb;
mod text;
mod zip_archive;

#[cfg(feature = "audio")]
mod audio;
#[cfg(feature = "epub")]
mod epub;
#[cfg(feature = "html")]
mod html;
#[cfg(feature = "image")]
mod image;
#[cfg(feature = "outlook")]
mod msg;
#[cfg(feature = "office")]
mod office;
#[cfg(feature = "pdf")]
mod pdf;
#[cfg(feature = "rss")]
mod rss;
#[cfg(feature = "html")]
mod wikipedia;

use std::sync::Arc;

use crate::convert::{DynConverter, PRIORITY_GENERIC, PRIORITY_SPECIFIC};
use crate::stream_info::StreamInfo;

pub(crate) fn builtins() -> Vec<(DynConverter, f32)> {
    #[cfg_attr(
        not(any(
            feature = "html",
            feature = "rss",
            feature = "epub",
            feature = "pdf",
            feature = "office",
            feature = "outlook",
            feature = "image",
            feature = "audio",
        )),
        allow(unused_mut)
    )]
    let mut converters: Vec<(DynConverter, f32)> = vec![
        (Arc::new(text::PlainTextConverter), PRIORITY_GENERIC),
        (Arc::new(zip_archive::ZipConverter), PRIORITY_GENERIC),
        (Arc::new(csv::CsvConverter), PRIORITY_SPECIFIC),
        (Arc::new(ipynb::IpynbConverter), PRIORITY_SPECIFIC),
    ];
    #[cfg(feature = "html")]
    {
        converters.push((Arc::new(html::HtmlConverter), PRIORITY_GENERIC));
        converters.push((Arc::new(wikipedia::WikipediaConverter), PRIORITY_SPECIFIC));
    }
    #[cfg(feature = "rss")]
    converters.push((Arc::new(rss::RssConverter), PRIORITY_SPECIFIC));
    #[cfg(feature = "epub")]
    converters.push((Arc::new(epub::EpubConverter), PRIORITY_SPECIFIC));
    #[cfg(feature = "pdf")]
    converters.push((Arc::new(pdf::PdfConverter), PRIORITY_SPECIFIC));
    #[cfg(feature = "office")]
    converters.push((Arc::new(office::OfficeConverter), PRIORITY_SPECIFIC));
    #[cfg(feature = "outlook")]
    converters.push((Arc::new(msg::MsgConverter), PRIORITY_SPECIFIC));
    #[cfg(feature = "image")]
    converters.push((Arc::new(image::ImageConverter), PRIORITY_SPECIFIC));
    #[cfg(feature = "audio")]
    converters.push((Arc::new(audio::AudioConverter), PRIORITY_SPECIFIC));
    converters
}

pub(crate) fn has_extension(info: &StreamInfo, extensions: &[&str]) -> bool {
    info.extension
        .as_deref()
        .is_some_and(|extension| extensions.contains(&extension))
}

fn media_type(info: &StreamInfo) -> Option<&str> {
    let mime = info.mimetype.as_deref()?;
    let mime = mime.split(';').next().unwrap_or(mime).trim();
    if mime.is_empty() {
        None
    } else {
        Some(mime)
    }
}

pub(crate) fn has_mime(info: &StreamInfo, mimes: &[&str]) -> bool {
    media_type(info).is_some_and(|mime| {
        mimes
            .iter()
            .any(|candidate| mime.eq_ignore_ascii_case(candidate))
    })
}

pub(crate) fn mime_starts_with(info: &StreamInfo, prefixes: &[&str]) -> bool {
    media_type(info).is_some_and(|mime| {
        let mime = mime.to_ascii_lowercase();
        prefixes
            .iter()
            .any(|prefix| mime.starts_with(&prefix.to_ascii_lowercase()))
    })
}
