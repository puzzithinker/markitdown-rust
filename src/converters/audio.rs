use std::io::Cursor;
use std::time::Duration;

use lofty::prelude::*;
use lofty::probe::Probe;

use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

const EXTENSIONS: &[&str] = &[".mp3", ".wav", ".flac", ".m4a"];

const MIMES: &[&str] = &[
    "audio/mpeg",
    "audio/wav",
    "audio/x-wav",
    "audio/flac",
    "audio/x-flac",
    "audio/mp4",
    "audio/x-m4a",
    "audio/m4a",
];

pub(crate) struct AudioConverter;

impl Converter for AudioConverter {
    fn name(&self) -> &'static str {
        "audio"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(info, EXTENSIONS) || has_mime(info, MIMES)
    }

    fn convert(
        &self,
        bytes: &[u8],
        _info: &StreamInfo,
        _ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let tagged = Probe::new(Cursor::new(bytes))
            .guess_file_type()
            .map_err(|err| Error::Message(err.to_string()))?
            .read()
            .map_err(|err| Error::Message(err.to_string()))?;
        let mut lines = vec![
            format!("- Container: {}", container_name(tagged.file_type())),
            format!(
                "- Duration: {}",
                format_duration(tagged.properties().duration())
            ),
        ];
        if let Some(tag) = tagged.primary_tag() {
            push_tag(&mut lines, "Title", tag.get_string(&ItemKey::TrackTitle));
            push_tag(&mut lines, "Artist", tag.get_string(&ItemKey::TrackArtist));
            push_tag(&mut lines, "Album", tag.get_string(&ItemKey::AlbumTitle));
        }
        Ok(DocumentConverterResult::new(lines.join("\n")))
    }
}

fn push_tag(lines: &mut Vec<String>, label: &str, value: Option<&str>) {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return;
    };
    lines.push(format!("- {label}: {value}"));
}

fn container_name(kind: lofty::file::FileType) -> String {
    match kind {
        lofty::file::FileType::Aac => "AAC".to_string(),
        lofty::file::FileType::Aiff => "AIFF".to_string(),
        lofty::file::FileType::Ape => "APE".to_string(),
        lofty::file::FileType::Flac => "FLAC".to_string(),
        lofty::file::FileType::Mpeg => "MPEG".to_string(),
        lofty::file::FileType::Mp4 => "MP4".to_string(),
        lofty::file::FileType::Mpc => "Musepack".to_string(),
        lofty::file::FileType::Opus => "Opus".to_string(),
        lofty::file::FileType::Vorbis => "Vorbis".to_string(),
        lofty::file::FileType::Speex => "Speex".to_string(),
        lofty::file::FileType::Wav => "WAV".to_string(),
        lofty::file::FileType::WavPack => "WavPack".to_string(),
        lofty::file::FileType::Custom(name) => name.to_string(),
        _ => format!("{kind:?}"),
    }
}

fn format_duration(duration: Duration) -> String {
    let total_ms = duration.as_millis();
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms / 60_000) % 60;
    let seconds = (total_ms / 1_000) % 60;
    let millis = total_ms % 1_000;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
    } else {
        format!("{minutes}:{seconds:02}.{millis:03}")
    }
}
