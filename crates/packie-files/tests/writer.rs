mod support;

use packie_files::{Charset, Error, LineEnding, OnExists, Record, read, write};
use support::strings;

#[test]
fn writer_orders_positions_fills_gaps_and_is_deterministic() {
    let mut template = strings(&["result", "sample"]);
    template.columns[0].position = Some(5);
    template.columns[0].header = Some("Value".into());
    template.columns[1].position = Some(1);
    template.line_ending = LineEnding::Lf;
    let records = vec![Record::new(&template, &["5.20", "SYN-001"]).unwrap()];
    let expected = b"sample,,,,Value\nSYN-001,,,,5.20\n";
    assert_eq!(write(&records, &template).unwrap(), expected);
    assert_eq!(write(&records, &template).unwrap(), expected);
    assert_eq!(read(expected, &template).unwrap().records, records);
    template.skip_lines = 5;
    template.file_name = Some("synthetic-{seq}.csv".into());
    template.on_exists = OnExists::Append;
    assert_eq!(write(&records, &template).unwrap(), expected);
}

#[test]
fn a_sparse_position_is_refused_instead_of_allocating_the_gap() {
    // Found by fuzzing (files_read): position u32::MAX asked for a 100 GB row.
    for header_row in [true, false] {
        let mut template = strings(&["id", "far"]);
        template.header_row = header_row;
        template.columns[1].position = Some(u32::MAX);
        assert!(matches!(write(&[], &template), Err(Error::Serialize)));
        let record = Record::new(&template, &["a", "b"]).unwrap();
        assert!(matches!(write(&[record], &template), Err(Error::Serialize)));
        template.columns[1].position = Some(4096);
        assert!(write(&[], &template).is_ok());
    }
}

#[test]
fn output_endings_and_boms_do_not_change_embedded_newlines() {
    for (line_ending, ending) in [
        (LineEnding::Crlf, "\r\n"),
        (LineEnding::Lf, "\n"),
        (LineEnding::Cr, "\r"),
    ] {
        for write_bom in [false, true] {
            let mut template = strings(&["id", "note"]);
            template.line_ending = line_ending;
            template.write_bom = write_bom;
            let records = vec![Record::new(&template, &["SYN-001", "a\rb\nc\r\nd"]).unwrap()];
            let bytes = write(&records, &template).unwrap();
            assert_eq!(bytes.starts_with(b"\xef\xbb\xbf"), write_bom);
            assert!(bytes.ends_with(ending.as_bytes()));
            assert_eq!(read(&bytes, &template).unwrap().records, records);
        }
    }
}

#[test]
fn nonstandard_ascii_and_unicode_quotes_are_doubled() {
    for quote in ["'", "§"] {
        let mut template = strings(&["id", "note"]);
        template.quote = Some(quote.into());
        template.line_ending = LineEnding::Lf;
        let value = format!("a,{quote}b{quote}\nc");
        let records = vec![Record::new(&template, &["SYN-001", &value]).unwrap()];
        let bytes = write(&records, &template).unwrap();
        let expected =
            format!("id,note\nSYN-001,{quote}a,{quote}{quote}b{quote}{quote}\nc{quote}\n");
        assert_eq!(bytes, expected.as_bytes());
        assert_eq!(read(&bytes, &template).unwrap().records, records);
    }
}

#[test]
fn disabled_quoting_rejects_unsafe_fields_and_empty_single_column_rows() {
    let mut template = strings(&["id", "note"]);
    template.quote = None;
    for unsafe_value in ["a,b", "a\nb", "a\rb"] {
        let record = Record::new(&template, &["SYN-001", unsafe_value]).unwrap();
        assert!(matches!(
            write(&[record], &template),
            Err(Error::QuotingDisabled { line: 2, column: 2 })
        ));
    }
    let record = Record::new(&template, &["SYN-001", "bare \" quote"]).unwrap();
    let bytes = write(std::slice::from_ref(&record), &template).unwrap();
    assert_eq!(read(&bytes, &template).unwrap().records, [record]);
    template.columns.truncate(1);
    let empty = Record::new(&template, &[""]).unwrap();
    assert!(matches!(
        write(&[empty], &template),
        Err(Error::QuotingDisabled { line: 2, column: 1 })
    ));
}

#[test]
fn writer_rejects_unrepresentable_values_and_headers() {
    for (charset, value) in [(Charset::Ascii, "é"), (Charset::Latin1, "λ")] {
        let mut template = strings(&["note"]);
        template.charset = charset;
        let record = Record::new(&template, &[value]).unwrap();
        assert!(
            matches!(write(&[record], &template), Err(Error::Unrepresentable { charset: actual }) if actual == charset)
        );
        template.columns[0].header = Some(value.into());
        assert!(
            matches!(write(&[], &template), Err(Error::Unrepresentable { charset: actual }) if actual == charset)
        );
    }
    let mut template = strings(&["note"]);
    template.charset = Charset::Latin1;
    template.line_ending = LineEnding::Lf;
    let record = Record::new(&template, &["Contrôle"]).unwrap();
    assert_eq!(write(&[record], &template).unwrap(), b"note\nContr\xf4le\n");
}

#[test]
fn first_field_bom_character_is_data_when_bom_output_is_disabled() {
    let mut template = strings(&["note", "other"]);
    template.header_row = false;
    let records = vec![Record::new(&template, &["\u{feff}data", "ok"]).unwrap()];
    let bytes = write(&records, &template).unwrap();
    assert_eq!(read(&bytes, &template).unwrap().records, records);
}

#[test]
fn output_never_exceeds_what_a_reader_of_the_template_accepts() {
    // Found by fuzzing (files_read round trip): `write` produced files `read` then refused.
    let mut template = strings(&["id", "note"]);
    template.max_file_bytes = 1024;
    template.write_bom = true;
    let row =
        |n: usize| Record::new(&template, &[&format!("SYN-{n:03}"), &"x".repeat(40)]).unwrap();
    let mut records = Vec::new();
    let mut last_fit = Vec::new();
    loop {
        records.push(row(records.len()));
        match write(&records, &template) {
            Ok(bytes) => last_fit = bytes,
            Err(Error::FileTooLarge { actual, limit }) => {
                assert_eq!(limit, 1024);
                assert!(actual > 1024, "{actual}");
                // The size reported is what the output would have been, BOM included.
                let mut larger = template.clone();
                larger.max_file_bytes = 4096;
                let full = write(&records, &larger).unwrap();
                assert_eq!(actual, full.len() as u64);
                assert!(matches!(
                    read(&full, &template),
                    Err(Error::FileTooLarge { actual: a, limit: 1024 }) if a == actual
                ));
                break;
            }
            Err(other) => panic!("unexpected {other}"),
        }
    }
    // The largest output that was written reads back with the same template.
    assert!(!last_fit.is_empty() && last_fit.len() <= 1024);
    records.pop();
    assert_eq!(read(&last_fit, &template).unwrap().records, records);
}
