//! Minimal CSV (RFC 4180) for dictionary and snippet import and export.
//! Quoted fields may hold commas, quotes ("" escapes) and line breaks, which
//! snippet text needs.

pub fn parse(text: &str) -> Vec<Vec<String>> {
    let text = text.trim_start_matches('\u{feff}');
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => quoted = false,
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                rows.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// One CSV line, CRLF terminated so Excel opens it cleanly.
pub fn row(fields: &[&str]) -> String {
    let cells: Vec<String> = fields
        .iter()
        .map(|f| {
            if f.contains([',', '"', '\n', '\r']) || f.starts_with(' ') || f.ends_with(' ') {
                format!("\"{}\"", f.replace('"', "\"\""))
            } else {
                f.to_string()
            }
        })
        .collect();
    format!("{}\r\n", cells.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_quoted_fields() {
        assert_eq!(
            parse("a,b\r\n\"c, d\",\"say \"\"hi\"\"\"\n"),
            vec![vec!["a", "b"], vec!["c, d", "say \"hi\""]]
        );
    }

    #[test]
    fn quoted_fields_keep_line_breaks() {
        assert_eq!(
            parse("sig,\"Best,\nSam\"\n"),
            vec![vec!["sig", "Best,\nSam"]]
        );
    }

    #[test]
    fn last_row_without_newline_and_byte_order_mark() {
        assert_eq!(
            parse("\u{feff}a,b\nc,d"),
            vec![vec!["a", "b"], vec!["c", "d"]]
        );
    }

    #[test]
    fn writes_quotes_only_when_needed() {
        assert_eq!(
            row(&["plain", "a,b", "say \"hi\""]),
            "plain,\"a,b\",\"say \"\"hi\"\"\"\r\n"
        );
        assert_eq!(parse(&row(&["x\ny", " pad "])), vec![vec!["x\ny", " pad "]]);
    }
}
