//! Delimited-file reading with a template derived from the first 22 input bytes and the file
//! contents from the rest, so every fixture preceded by a 22-byte prefix is a seed. Byte 0
//! picks delimiter, quote, header row and skip lines; byte 1 charset, line ending and record
//! mode; byte 2 the column count and group-by column; byte 3 the size limit; then three bytes
//! per column choose its type, position, format and required flag. A column's `header` is
//! taken from the matching cell of the file's first line so header matching succeeds.
//! A file that reads is grouped, written back and read again.
#![no_main]

use libfuzzer_sys::fuzz_target;
use packie_files::{
    Charset, Column, ColumnType, Document, LineEnding, RecordMode, Template, read, write,
};

const PREFIX: usize = 22;
const DELIMITERS: [&str; 4] = [",", ";", "|", "\t"];
const POSITIONS: [u32; 4] = [1, 3, 6, u32::MAX];
const DATE_FORMATS: [&str; 3] = ["yyyy-MM-dd", "dd/MM/yyyy", "yyyyMMdd"];
const TIME_FORMATS: [&str; 3] = ["HH:mm:ss", "HH:mm", "HHmmss"];
const DATETIME_FORMATS: [&str; 3] = ["yyyy-MM-dd HH:mm:ss", "yyyyMMddHHmmss", "dd/MM/yyyy HH:mm"];

fn column(index: usize, control: [u8; 3], header: Option<&str>) -> Column {
    let column_type = match control[0] % 7 {
        0 => ColumnType::String,
        1 => ColumnType::Integer,
        2 => ColumnType::Decimal,
        3 => ColumnType::Date,
        4 => ColumnType::Time,
        5 => ColumnType::Datetime,
        _ => ColumnType::Boolean {
            true_literal: "Y".into(),
            false_literal: "N".into(),
        },
    };
    let format = match column_type {
        ColumnType::Date => Some(DATE_FORMATS[usize::from(control[1] >> 4) % 3]),
        ColumnType::Time => Some(TIME_FORMATS[usize::from(control[1] >> 4) % 3]),
        ColumnType::Datetime => Some(DATETIME_FORMATS[usize::from(control[1] >> 4) % 3]),
        _ => None,
    };
    Column {
        name: format!("c{index}"),
        header: header.map(str::to_owned),
        position: (control[1] & 1 != 0).then(|| POSITIONS[usize::from(control[1] >> 1) % 4]),
        column_type,
        format: format.map(str::to_owned),
        required: control[2] & 1 != 0,
    }
}

fn template(prefix: &[u8; PREFIX], payload: &[u8]) -> Template {
    let delimiter = DELIMITERS[usize::from(prefix[0] & 3)];
    let columns = 1 + usize::from(prefix[2] % 6);
    let first_line = payload
        .split(|byte| matches!(byte, b'\r' | b'\n'))
        .next()
        .map(String::from_utf8_lossy)
        .unwrap_or_default();
    let mut cells = first_line.split(delimiter);
    let columns = (0..columns)
        .map(|index| {
            let header = cells
                .next()
                .filter(|cell| !cell.is_empty() && prefix[4 + index * 3 + 2] & 2 != 0);
            let control = [
                prefix[4 + index * 3],
                prefix[5 + index * 3],
                prefix[6 + index * 3],
            ];
            column(index, control, header)
        })
        .collect::<Vec<_>>();
    let mode = match (prefix[1] >> 4) % 3 {
        0 => RecordMode::OnePerLine,
        1 => RecordMode::Grouped,
        _ => RecordMode::File,
    };
    Template {
        delimiter: delimiter.into(),
        quote: match (prefix[0] >> 2) & 3 {
            0 => None,
            1 => Some("'".into()),
            _ => Some("\"".into()),
        },
        header_row: prefix[0] & 0x10 != 0,
        skip_lines: u32::from(prefix[0] >> 5),
        charset: match prefix[1] % 3 {
            0 => Charset::Utf8,
            1 => Charset::Latin1,
            _ => Charset::Ascii,
        },
        line_ending: match (prefix[1] >> 2) % 3 {
            0 => LineEnding::Crlf,
            1 => LineEnding::Lf,
            _ => LineEnding::Cr,
        },
        group_by: (mode == RecordMode::Grouped)
            .then(|| format!("c{}", usize::from(prefix[2] >> 3) % columns.len())),
        record_mode: mode,
        columns,
        max_file_bytes: [1024, 4096, 65_536, 1 << 30][usize::from(prefix[3] & 3)],
        ..Template::default()
    }
}

fn check(document: &Document, template: &Template) {
    let Ok(messages) = document.messages(template) else {
        return;
    };
    for message in messages {
        for record in message {
            for field in record.fields() {
                let _ = (
                    field.name(),
                    field.value().as_str(),
                    field.value().as_boolean(),
                );
            }
            let _ = record.get("col.c0");
            let _ = record.get("col[1]");
        }
    }
    let Ok(bytes) = write(&document.records, template) else {
        return;
    };
    // Skipped lines and the size limit are source-only settings: the writer ignores them. A
    // written file may still be refused on reading (a sparse template writes several empty
    // header cells, which the reader calls ambiguous); only a successful read must match.
    let readback = Template {
        skip_lines: 0,
        max_file_bytes: 1 << 30,
        ..template.clone()
    };
    if let Ok(again) = read(&bytes, &readback) {
        assert_eq!(&again, document, "{template:?}\n{bytes:?}");
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((prefix, payload)) = data.split_first_chunk::<PREFIX>() else {
        return;
    };
    let template = template(prefix, payload);
    if let Ok(document) = read(payload, &template) {
        check(&document, &template);
    }
});
