use std::io::{Cursor, Read};
use std::path::Path;

use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

pub(crate) struct ZipConverter;

impl Converter for ZipConverter {
    fn name(&self) -> &'static str {
        "zip"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(info, &[".zip"]) || has_mime(info, &["application/zip"])
    }

    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        if ctx.depth >= ctx.options.max_zip_depth {
            return Err(Error::UnsupportedFormat("ZIP nesting limit reached".into()));
        }
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|err| Error::Message(err.to_string()))?;
        if archive.len() > ctx.options.max_zip_entries {
            return Err(Error::Message(format!(
                "ZIP has {} entries, above the limit of {}",
                archive.len(),
                ctx.options.max_zip_entries
            )));
        }

        let label = info
            .url
            .as_deref()
            .or(info.local_path.as_deref())
            .or(info.filename.as_deref())
            .unwrap_or("(unknown)");
        let mut markdown = format!("Content from the zip file `{label}`:\n\n");
        let mut uncompressed = 0u64;

        for index in 0..archive.len() {
            let mut file = match archive.by_index(index) {
                Ok(file) => file,
                // `by_index` refuses an encrypted member before `encrypted()` can be read.
                Err(zip::result::ZipError::UnsupportedArchive(
                    zip::result::ZipError::PASSWORD_REQUIRED,
                ))
                | Err(zip::result::ZipError::InvalidPassword) => continue,
                Err(err) => return Err(Error::Message(err.to_string())),
            };
            if file.is_dir() || file.encrypted() {
                continue;
            }
            let name = file.name().replace('\\', "/");
            if file.enclosed_name().is_none() || name.split('/').any(|part| part == "..") {
                continue;
            }
            let size = file.size();
            uncompressed = uncompressed.saturating_add(size);
            if uncompressed > ctx.options.max_zip_uncompressed_bytes {
                return Err(Error::Message(
                    "ZIP uncompressed size exceeds the configured limit".into(),
                ));
            }
            let mut member = Vec::new();
            file.by_ref()
                .take(size.saturating_add(1))
                .read_to_end(&mut member)
                .map_err(Error::Io)?;
            // `take` stops after one extra byte, including when the declared size is 0.
            if member.len() as u64 > size {
                return Err(Error::Message(
                    "ZIP member is larger than its declared size".into(),
                ));
            }
            drop(file);

            let member_info = StreamInfo {
                extension: Path::new(&name)
                    .extension()
                    .and_then(|ext| crate::stream_info::some_extension(&ext.to_string_lossy())),
                filename: Path::new(&name)
                    .file_name()
                    .map(|filename| filename.to_string_lossy().into_owned()),
                ..StreamInfo::default()
            };
            match ctx
                .engine
                .convert_inner(&member, member_info, None, ctx.depth + 1)
            {
                Ok(result) => {
                    markdown.push_str(&format!("## File: {name}\n\n{}\n\n", result.markdown));
                }
                Err(Error::UnsupportedFormat(_) | Error::FileConversion(_)) => {}
                Err(error) => return Err(error),
            }
        }

        Ok(DocumentConverterResult::new(markdown.trim_end()))
    }
}
