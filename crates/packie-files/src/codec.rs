use crate::lexer::{Row, parse_rows};
use crate::{Charset, Document, Error, LineEnding, Record, Template};

/// Parse a complete file atomically. `max_file_bytes` includes the BOM and skipped lines.
pub fn read(bytes: &[u8], template: &Template) -> Result<Document, Error> {
    template.check()?;
    if bytes.len() as u64 > template.max_file_bytes {
        return Err(Error::FileTooLarge {
            actual: bytes.len() as u64,
            limit: template.max_file_bytes,
        });
    }
    let text = decode(bytes, template.charset)?;
    let (text, line) = skip_lines(&text, template.skip_lines);
    let rows = parse_rows(
        text,
        template.delimiter_char()?,
        template.quote_char(),
        line,
    )?;
    build_document(rows, template)
}
fn decode(bytes: &[u8], charset: Charset) -> Result<String, Error> {
    match charset {
        Charset::Latin1 => Ok(bytes.iter().copied().map(char::from).collect()),
        Charset::Ascii => match bytes.iter().position(|b| !b.is_ascii()) {
            Some(offset) => Err(Error::Encoding { charset, offset }),
            None => Ok(bytes.iter().copied().map(char::from).collect()),
        },
        Charset::Utf8 => {
            let bom = usize::from(bytes.starts_with(b"\xef\xbb\xbf")) * 3;
            std::str::from_utf8(&bytes[bom..])
                .map(str::to_owned)
                .map_err(|e| Error::Encoding {
                    charset,
                    offset: bom + e.valid_up_to(),
                })
        }
    }
}
fn skip_lines(mut text: &str, count: u32) -> (&str, usize) {
    let mut line = 1;
    for _ in 0..count {
        if let Some(at) = text.find(['\r', '\n']) {
            let len = if text[at..].starts_with("\r\n") { 2 } else { 1 };
            text = &text[at + len..];
            line += 1;
        } else {
            text = "";
            break;
        }
    }
    (text, line)
}
fn build_document(rows: Vec<Row>, template: &Template) -> Result<Document, Error> {
    let mut rows = rows.into_iter();
    let header = if template.header_row {
        rows.next()
    } else {
        None
    };
    let mut indices = template.positions();
    if let Some(header) = &header {
        indices = header_indices(header, template)?;
    }
    let expected = header.as_ref().map(|r| r.fields.len());
    let minimum = indices.iter().copied().max().unwrap_or(0) + 1;
    let mut width = expected;
    let mut records = Vec::new();
    let empty = Record::blank(template);
    for row in rows {
        let expected = *width.get_or_insert(row.fields.len().max(minimum));
        if row.fields.len() != expected {
            return Err(Error::Ragged {
                line: row.line,
                column: row.fields.len().min(expected) + 1,
                expected,
                actual: row.fields.len(),
            });
        }
        let mut record = empty.clone();
        for (column, index) in template.columns.iter().zip(&indices) {
            let text = row.fields.get(*index).ok_or_else(|| Error::MissingColumn {
                line: row.line,
                column: column.name.clone(),
            })?;
            record
                .set_column(&column.name, text)
                .map_err(|_| Error::InvalidValue {
                    line: row.line,
                    column: column.name.clone(),
                })?;
        }
        records.push(record);
    }
    Ok(Document { records })
}
fn header_indices(header: &Row, template: &Template) -> Result<Vec<usize>, Error> {
    let normalized: Vec<_> = header.fields.iter().map(|s| s.to_lowercase()).collect();
    let mut seen = std::collections::HashSet::new();
    for (i, name) in normalized.iter().enumerate() {
        if (!name.is_empty()
            || template
                .columns
                .iter()
                .any(|c| c.header.as_deref() == Some("")))
            && !seen.insert(name)
        {
            return Err(Error::DuplicateHeader {
                line: header.line,
                column: i + 1,
            });
        }
    }
    template
        .columns
        .iter()
        .zip(template.positions())
        .map(|(column, position)| {
            let name = column
                .header
                .as_ref()
                .unwrap_or(&column.name)
                .to_lowercase();
            normalized
                .iter()
                .position(|s| s == &name)
                .or_else(|| {
                    (column.header.is_none()
                        && column.position.is_some()
                        && position < header.fields.len())
                    .then_some(position)
                })
                .ok_or_else(|| Error::MissingColumn {
                    line: header.line,
                    column: column.name.clone(),
                })
        })
        .collect()
}
/// Widest row `write` builds. Positions are only bounded by `u32`; without a limit a sparse
/// template (position 4294967295) would ask for gigabytes of empty cells per row.
const MAX_ROW_WIDTH: usize = 4096;

/// Encode records deterministically. Header and values follow one-based positions;
/// skipped source lines are not emitted. A template whose last position exceeds
/// `MAX_ROW_WIDTH` fails with `Error::Serialize`; output larger than `max_file_bytes`
/// (BOM included, as `read` counts it) fails with `Error::FileTooLarge`, so `read` with
/// the same template accepts everything `write` produces. This performs no transport
/// operations.
pub fn write(records: &[Record], template: &Template) -> Result<Vec<u8>, Error> {
    template.check()?;
    let positions = template.positions();
    let width = positions.iter().copied().max().unwrap_or(0) + 1;
    if width > MAX_ROW_WIDTH {
        return Err(Error::Serialize);
    }
    let mut rows = Vec::new();
    if template.header_row {
        let mut header = vec![String::new(); width];
        for (c, i) in template.columns.iter().zip(&positions) {
            header[*i].clone_from(c.header.as_ref().unwrap_or(&c.name));
        }
        rows.push(header);
    }
    for (i, record) in records.iter().enumerate() {
        let mut row = vec![String::new(); width];
        for (c, index) in template.columns.iter().zip(&positions) {
            let value = record
                .column_text(&c.name)
                .ok_or_else(|| Error::MissingColumn {
                    line: i + 1,
                    column: c.name.clone(),
                })?;
            if !crate::model::valid_text(c, value) {
                return Err(Error::InvalidValue {
                    line: i + 1,
                    column: c.name.clone(),
                });
            }
            row[*index] = value.into();
        }
        rows.push(row);
    }
    let text = serialize(&rows, template)?;
    let mut bytes = encode(&text, template.charset)?;
    if template.write_bom {
        bytes.splice(..0, [0xef, 0xbb, 0xbf]);
    }
    let actual = bytes.len() as u64;
    if actual > template.max_file_bytes {
        return Err(Error::FileTooLarge {
            actual,
            limit: template.max_file_bytes,
        });
    }
    Ok(bytes)
}
fn serialize(rows: &[Vec<String>], template: &Template) -> Result<String, Error> {
    let delimiter = template.delimiter_char()?;
    let quote = template.quote_char();
    let ending = match template.line_ending {
        LineEnding::Crlf => "\r\n",
        LineEnding::Lf => "\n",
        LineEnding::Cr => "\r",
    };
    // Without a BOM to write, a leading U+FEFF in the data would be mistaken for one on reading.
    let initial_bom = !template.write_bom
        && template.charset == Charset::Utf8
        && rows
            .first()
            .and_then(|row| row.first())
            .is_some_and(|s| s.starts_with('\u{feff}'));
    let mut output = String::new();
    for (i, row) in rows.iter().enumerate() {
        for (j, value) in row.iter().enumerate() {
            if j != 0 {
                output.push(delimiter);
            }
            let needs_quote = value.contains([delimiter, '\r', '\n'])
                || quote.is_some_and(|q| value.contains(q))
                || (row.len() == 1 && value.is_empty())
                || (initial_bom && i == 0 && j == 0);
            if needs_quote {
                let q = quote.ok_or(Error::QuotingDisabled {
                    line: i + 1,
                    column: j + 1,
                })?;
                append_quoted(&mut output, value, q);
            } else {
                output.push_str(value);
            }
        }
        output.push_str(ending);
    }
    Ok(output)
}
fn append_quoted(output: &mut String, value: &str, quote: char) {
    output.push(quote);
    for ch in value.chars() {
        output.push(ch);
        if ch == quote {
            output.push(ch);
        }
    }
    output.push(quote);
}
fn encode(text: &str, charset: Charset) -> Result<Vec<u8>, Error> {
    if charset == Charset::Utf8 {
        return Ok(text.as_bytes().to_vec());
    }
    text.chars()
        .map(|ch| {
            u8::try_from(ch)
                .ok()
                .filter(|byte| charset != Charset::Ascii || byte.is_ascii())
                .ok_or(Error::Unrepresentable { charset })
        })
        .collect()
}
