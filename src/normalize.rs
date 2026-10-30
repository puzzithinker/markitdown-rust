/// Strip trailing whitespace on each line and collapse runs of 3+ newlines to 2.
pub fn normalize_markdown(markdown: &str) -> String {
    let mut trimmed = String::with_capacity(markdown.len());
    for (index, line) in markdown.split('\n').enumerate() {
        if index > 0 {
            trimmed.push('\n');
        }
        let line = line.strip_suffix('\r').unwrap_or(line);
        trimmed.push_str(line.trim_end());
    }
    let mut out = String::with_capacity(trimmed.len());
    let mut newlines = 0u8;
    for ch in trimmed.chars() {
        if ch == '\n' {
            newlines = newlines.saturating_add(1);
            if newlines <= 2 {
                out.push('\n');
            }
        } else {
            newlines = 0;
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::normalize_markdown;

    #[test]
    fn collapses_blank_lines_and_trims_trailing_space() {
        assert_eq!(normalize_markdown("a  \r\n\n\n\nb \n"), "a\n\nb\n");
    }

    #[test]
    fn collapses_a_long_blank_run_in_one_pass() {
        let input = format!("start\n{}\nend  \n", "\n".repeat(40));
        assert_eq!(normalize_markdown(&input), "start\n\nend\n");
    }

    #[test]
    fn keeps_two_newlines_and_internal_spacing() {
        assert_eq!(normalize_markdown("a\n\nb\n\n"), "a\n\nb\n\n");
        assert_eq!(normalize_markdown("a  b\t\n"), "a  b\n");
    }
}
