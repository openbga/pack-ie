mod support;

use packie_files::{Charset, Document, Error, RecordMode, read};
use support::strings;

#[test]
fn latin1_semicolon_headers_match_case_insensitively_without_bom() {
    let mut template = strings(&["id", "label", "result"]);
    template.delimiter = ";".into();
    template.charset = Charset::Latin1;
    for (column, header) in template
        .columns
        .iter_mut()
        .zip(["IDENTIFIANT", "LIBELLÉ", "RÉSULTAT"])
    {
        column.header = Some(header.into());
    }
    let document = read(include_bytes!("fixtures/latin1.csv"), &template).unwrap();
    assert_eq!(
        document.records[0].get("col.label"),
        Some("Contrôle synthétique")
    );
    assert_eq!(document.records[0].get("col.result"), Some("12.50"));
}

#[test]
fn quoted_fields_preserve_embedded_delimiter_line_endings_and_doubled_quotes() {
    let template = strings(&["id", "note", "flag"]);
    let document = read(include_bytes!("fixtures/quoted.csv"), &template).unwrap();
    assert_eq!(document.records.len(), 2);
    assert_eq!(
        document.records[0].get("col.note"),
        Some("delimiter, quote \"yes\"\r\nand another line")
    );
    assert_eq!(document.records[1].get("col.note"), Some(""));
    assert_eq!(document.records[1].get("col.flag"), Some("N"));
}

#[test]
fn explicit_headers_override_positions_and_missing_default_names_fall_back() {
    let mut template = strings(&["sample", "result"]);
    template.columns[0].header = Some("SPECIMEN".into());
    template.columns[0].position = Some(2);
    template.columns[1].position = Some(3);
    let document = read(
        b"specimen,unused,instrument_value\nSYN-001,ignore,5.20\n",
        &template,
    )
    .unwrap();
    assert_eq!(document.records[0].get("col.sample"), Some("SYN-001"));
    assert_eq!(document.records[0].get("col.result"), Some("5.20"));
    assert!(
        matches!(read(b"wrong,sample,result\nx,SYN-001,5.2\n", &template), Err(Error::MissingColumn { column, line: 1 }) if column == "sample")
    );
    assert!(matches!(
        read(b"SPECIMEN,specimen,result\na,b,c\n", &template),
        Err(Error::DuplicateHeader { line: 1, column: 2 })
    ));
}

#[test]
fn explicit_and_implicit_positions_read_headerless_files() {
    let mut template = strings(&["result", "unit", "sample"]);
    template.header_row = false;
    template.columns[0].position = Some(3);
    template.columns[2].position = Some(1);
    let document = read(b"SYN-001,unused,5.20,mmol/L\n", &template).unwrap();
    assert_eq!(document.records[0].get("col.sample"), Some("SYN-001"));
    assert_eq!(document.records[0].get("col.result"), Some("5.20"));
    assert_eq!(document.records[0].get("col.unit"), Some("mmol/L"));
}

#[test]
fn skip_lines_counts_physical_lines_even_when_quotes_are_unbalanced() {
    let mut template = strings(&["id", "note"]);
    template.skip_lines = 2;
    let document = read(
        b"instrument \"banner\r\nsecond physical line\rid,note\nSYN-001,ok",
        &template,
    )
    .unwrap();
    assert_eq!(document.records[0].get("col.id"), Some("SYN-001"));
    assert!(matches!(
        read(b"banner\nsecond\nid,note\na,b\nb\n", &template),
        Err(Error::Ragged { line: 5, .. })
    ));
    assert!(read(b"one line", &template).unwrap().records.is_empty());
}

#[test]
fn readers_accept_every_line_ending_and_optional_utf8_bom() {
    let template = strings(&["id", "note"]);
    for ending in ["\r\n", "\n", "\r"] {
        for bom in ["", "\u{feff}"] {
            let text = format!("{bom}id,note{ending}{ending}SYN-001,ok{ending}SYN-002,last");
            let document = read(text.as_bytes(), &template).unwrap();
            assert_eq!(document.records.len(), 2);
            assert_eq!(document.records[1].get("col.note"), Some("last"));
        }
    }
    let mixed = read(b"id,note\r\nSYN-001,a\rSYN-002,b\n", &template).unwrap();
    assert_eq!(mixed.records.len(), 2);
}

#[test]
fn groups_are_consecutive_runs_of_exact_text_and_modes_preserve_rows() {
    let mut template = strings(&["sample_id", "test", "result"]);
    template.record_mode = RecordMode::Grouped;
    template.group_by = Some("sample_id".into());
    let document = read(include_bytes!("fixtures/grouped.csv"), &template).unwrap();
    let groups = document.messages(&template).unwrap();
    assert_eq!(
        groups.iter().map(|group| group.len()).collect::<Vec<_>>(),
        [2, 1, 2]
    );
    assert_eq!(groups[1][0].get("col.sample_id"), Some("1"));
    assert_eq!(groups[2][0].get("col.sample_id"), Some("001"));
    template.record_mode = RecordMode::OnePerLine;
    template.group_by = None;
    assert_eq!(document.messages(&template).unwrap().len(), 5);
    template.record_mode = RecordMode::File;
    assert_eq!(document.messages(&template).unwrap()[0], document.records);
}

#[test]
fn empty_documents_have_defined_message_counts() {
    let document = Document::default();
    let mut template = strings(&["id"]);
    assert!(document.messages(&template).unwrap().is_empty());
    template.record_mode = RecordMode::Grouped;
    template.group_by = Some("id".into());
    assert!(document.messages(&template).unwrap().is_empty());
    template.record_mode = RecordMode::File;
    template.group_by = None;
    assert_eq!(document.messages(&template).unwrap(), vec![&[][..]]);
}

#[test]
fn malformed_files_fail_atomically_with_physical_coordinates() {
    let template = strings(&["id", "result"]);
    assert!(matches!(
        read(include_bytes!("fixtures/ragged.csv"), &template),
        Err(Error::Ragged {
            line: 3,
            column: 2,
            expected: 2,
            actual: 1
        })
    ));
    assert!(matches!(
        read(include_bytes!("fixtures/truncated.csv"), &template),
        Err(Error::Truncated { line: 4, column: 2 })
    ));
    for bytes in [b"id,result\na,b\"c\n".as_slice(), b"id,result\na,\"b\"x\n"] {
        assert!(matches!(
            read(bytes, &template),
            Err(Error::Syntax { line: 2, column: 2 })
        ));
    }
    assert!(matches!(
        read(b"id,result\n\"a\nb\",c\nd\n", &template),
        Err(Error::Ragged { line: 4, .. })
    ));
}

#[test]
fn charset_errors_report_offsets_and_latin1_never_assumes_windows1252() {
    let mut template = strings(&["id", "note"]);
    let fixture = include_bytes!("fixtures/wrong-charset.csv");
    assert!(matches!(
        read(fixture, &template),
        Err(Error::Encoding {
            charset: Charset::Utf8,
            offset: 16
        })
    ));
    template.charset = Charset::Ascii;
    assert!(matches!(
        read(fixture, &template),
        Err(Error::Encoding {
            charset: Charset::Ascii,
            offset: 16
        })
    ));
    template.charset = Charset::Latin1;
    let document = read(fixture, &template).unwrap();
    assert_eq!(document.records[0].get("col.note"), Some("ÿÀ¯"));
    let document = read(b"id,note\na,\x80\n", &template).unwrap();
    assert_eq!(document.records[0].get("col.note"), Some("\u{80}"));
}

#[test]
fn byte_limit_includes_bom_and_skipped_lines_and_allows_exact_boundary() {
    let mut template = strings(&["id"]);
    template.max_file_bytes = 1024;
    let mut bytes = b"\xef\xbb\xbfid\n".to_vec();
    bytes.resize(1024, b'x');
    assert_eq!(read(&bytes, &template).unwrap().records.len(), 1);
    bytes.push(b'x');
    assert!(matches!(
        read(&bytes, &template),
        Err(Error::FileTooLarge {
            actual: 1025,
            limit: 1024
        })
    ));
    template.skip_lines = 1;
    assert!(matches!(
        read(&bytes, &template),
        Err(Error::FileTooLarge { .. })
    ));
}
