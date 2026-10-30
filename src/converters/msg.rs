use std::path::Path;

use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::html::convert_html_str;
use crate::converters::{has_extension, has_mime};
use crate::error::{Error, Result};
use crate::stream_info::StreamInfo;

pub(crate) struct MsgConverter;

impl Converter for MsgConverter {
    fn name(&self) -> &'static str {
        "outlook"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(info, &[".msg"]) || has_mime(info, &["application/vnd.ms-outlook"])
    }

    fn convert(
        &self,
        bytes: &[u8],
        _info: &StreamInfo,
        ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let message = msg_parser::Outlook::from_slice(bytes)
            .map_err(|err| Error::Message(err.to_string()))?;
        let subject = one_line(&message.subject);
        let mut markdown = String::from("# Email Message\n\n");
        push_field(&mut markdown, "From", &format_person(&message.sender));
        push_field(&mut markdown, "To", &format_people(&message.to));
        push_field(&mut markdown, "Cc", &format_people(&message.cc));
        let date = if !message.message_delivery_time.is_empty() {
            message.message_delivery_time.as_str()
        } else {
            message.client_submit_time.as_str()
        };
        push_field(&mut markdown, "Date", &one_line(date));
        push_field(&mut markdown, "Subject", &subject);
        markdown.push_str("\n## Content\n\n");

        let plain = message.body.trim();
        if !plain.is_empty() {
            markdown.push_str(plain);
        } else if !message.html.trim().is_empty() {
            let html = convert_html_str(&message.html, ctx.options.keep_data_uris)?;
            markdown.push_str(html.markdown.trim());
        }

        if !message.attachments.is_empty() {
            markdown.push_str("\n\n## Attachments\n");
            for attachment in &message.attachments {
                let name = attachment_name(attachment);
                markdown.push_str(&format!("\n### {name}\n"));
                if attachment.payload_bytes.is_empty() {
                    continue;
                }
                if attachment.is_embedded_message() && ctx.depth >= 4 {
                    continue;
                }
                let mut info = StreamInfo::new();
                if !attachment.extension.is_empty() {
                    info = info.with_extension(&attachment.extension);
                } else if let Some(extension) = Path::new(&name).extension() {
                    info = info.with_extension(extension.to_string_lossy().as_ref());
                }
                info.filename = Some(name);
                if !attachment.mime_tag.is_empty() {
                    info = info.with_mimetype(&attachment.mime_tag);
                }
                let depth = if attachment.is_embedded_message() {
                    ctx.depth.saturating_add(1)
                } else {
                    ctx.depth
                };
                match ctx
                    .engine
                    .convert_inner(&attachment.payload_bytes, info, None, depth)
                {
                    Ok(result) => {
                        if !result.markdown.trim().is_empty() {
                            markdown.push('\n');
                            markdown.push_str(result.markdown.trim());
                            markdown.push('\n');
                        }
                    }
                    Err(Error::UnsupportedFormat(_) | Error::FileConversion(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        }

        let mut result = DocumentConverterResult::new(markdown.trim_end());
        result.title = if subject.is_empty() {
            None
        } else {
            Some(subject)
        };
        Ok(result)
    }
}

fn push_field(markdown: &mut String, label: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    markdown.push_str(&format!("**{label}:** {value}\n"));
}

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_person(person: &msg_parser::Person) -> String {
    match (person.name.is_empty(), person.email.is_empty()) {
        (true, true) => String::new(),
        (false, true) => one_line(&person.name),
        (true, false) => one_line(&person.email),
        (false, false) => format!("{} <{}>", one_line(&person.name), one_line(&person.email)),
    }
}

fn format_people(people: &[msg_parser::Person]) -> String {
    people
        .iter()
        .map(format_person)
        .filter(|person| !person.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
}

fn attachment_name(attachment: &msg_parser::Attachment) -> String {
    for candidate in [
        attachment.long_file_name.as_str(),
        attachment.display_name.as_str(),
        attachment.file_name.as_str(),
    ] {
        if !candidate.is_empty() {
            return candidate.to_string();
        }
    }
    "attachment".to_string()
}
