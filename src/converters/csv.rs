use crate::charset::decode_text;
use crate::convert::{ConvertContext, Converter, DocumentConverterResult};
use crate::converters::{has_extension, has_mime};
use crate::error::Result;
use crate::stream_info::StreamInfo;

pub(crate) struct CsvConverter;

impl Converter for CsvConverter {
    fn name(&self) -> &'static str {
        "csv"
    }

    fn accepts(&self, info: &StreamInfo, _bytes: &[u8]) -> bool {
        has_extension(info, &[".csv"]) || has_mime(info, &["text/csv", "application/csv"])
    }

    fn convert(
        &self,
        bytes: &[u8],
        info: &StreamInfo,
        _ctx: &ConvertContext<'_>,
    ) -> Result<DocumentConverterResult> {
        let content = decode_text(bytes, info.charset.as_deref());
        let mut rows: Vec<Vec<String>> = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(content.as_bytes())
            .records()
            .map(|record| {
                record.map(|record| record.iter().map(str::to_string).collect::<Vec<_>>())
            })
            .collect::<std::result::Result<_, _>>()
            .map_err(|err| crate::error::Error::Message(err.to_string()))?;
        trim_outer_blank_rows(&mut rows);
        if rows.is_empty() {
            return Ok(DocumentConverterResult::new(String::new()));
        }
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        for row in &mut rows {
            row.resize(columns, String::new());
        }
        let mut lines = Vec::with_capacity(rows.len() + 1);
        lines.push(format!("| {} |", join_cells(&rows[0])));
        lines.push(format!("| {} |", vec!["---"; columns].join(" | ")));
        for row in rows.iter().skip(1) {
            lines.push(format!("| {} |", join_cells(row)));
        }
        Ok(DocumentConverterResult::new(lines.join("\n")))
    }
}

fn join_cells(row: &[String]) -> String {
    row.iter()
        .map(|cell| escape_cell(cell))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn escape_cell(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    let mut out = String::with_capacity(value.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' {
            let start = index;
            while index < chars.len() && chars[index] == '\\' {
                index += 1;
            }
            let slashes = index - start;
            if index < chars.len() && chars[index] == '|' {
                for _ in 0..(slashes * 2) {
                    out.push('\\');
                }
                out.push('\\');
                out.push('|');
                index += 1;
            } else {
                for _ in 0..slashes {
                    out.push('\\');
                }
            }
        } else if chars[index] == '|' {
            out.push('\\');
            out.push('|');
            index += 1;
        } else if chars[index] == '\r' || chars[index] == '\n' {
            if chars[index] == '\r' && index + 1 < chars.len() && chars[index + 1] == '\n' {
                index += 2;
            } else {
                index += 1;
            }
            out.push(' ');
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// Drop blank rows before the header, immediately under it, and at the end.
///
/// A blank row has no cells, or only empty cells. The `csv` crate yields `[""]`
/// for a blank line, which is the row Python's reader yields as `[]`.
fn trim_outer_blank_rows(rows: &mut Vec<Vec<String>>) {
    let mut start = 0;
    while start < rows.len() && is_blank_row(&rows[start]) {
        start += 1;
    }
    if start == rows.len() {
        rows.clear();
        return;
    }
    let header = start;
    start += 1;
    while start < rows.len() && is_blank_row(&rows[start]) {
        start += 1;
    }
    let mut end = rows.len();
    while end > start && is_blank_row(&rows[end - 1]) {
        end -= 1;
    }
    rows.truncate(end);
    rows.drain(header + 1..start);
    rows.drain(..header);
}

fn is_blank_row(row: &[String]) -> bool {
    row.iter().all(|cell| cell.is_empty())
}
