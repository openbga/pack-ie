mod support;

use packie_files::{Charset, ColumnType, LineEnding, Record, Template, read, write};
use proptest::prelude::*;
use support::{boolean, column, strings, temporal, with_columns};

fn dialect(seed: u16) -> Template {
    let mut template = strings(&["first", "second", "third"]);
    template.delimiter = [",", ";", "|", "\t"][usize::from(seed % 4)].into();
    template.quote = Some(["\"", "'", "§"][usize::from((seed / 4) % 3)].into());
    template.header_row = (seed / 12).is_multiple_of(2);
    template.charset =
        [Charset::Utf8, Charset::Latin1, Charset::Ascii][usize::from((seed / 24) % 3)];
    // An ASCII output cannot represent the Unicode quote itself.
    if template.charset == Charset::Ascii && template.quote.as_deref() == Some("§") {
        template.quote = Some("'".into());
    }
    template.line_ending =
        [LineEnding::Crlf, LineEnding::Lf, LineEnding::Cr][usize::from((seed / 72) % 3)];
    template.write_bom = template.charset == Charset::Utf8 && (seed / 216).is_multiple_of(2);
    if (seed / 432).is_multiple_of(2) {
        template.columns[0].position = Some(5);
        template.columns[1].position = Some(1);
        template.columns[2].position = Some(3);
    }
    template
}

fn representable(text: &str, charset: Charset) -> String {
    text.chars()
        .filter(|ch| match charset {
            Charset::Utf8 => true,
            Charset::Latin1 => u32::from(*ch) <= 255,
            Charset::Ascii => ch.is_ascii(),
        })
        .collect()
}

fn field() -> impl Strategy<Value = String> {
    prop::collection::vec(
        prop_oneof![
            Just(','),
            Just(';'),
            Just('|'),
            Just('\t'),
            Just('"'),
            Just('\''),
            Just('\r'),
            Just('\n'),
            Just(' '),
            Just('é'),
            Just('§'),
            Just('λ'),
            Just('\u{feff}'),
            any::<char>(),
        ],
        0..40,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(192))]

    #[test]
    fn representable_fields_roundtrip_across_dialects(
        seed in any::<u16>(), rows in prop::collection::vec((field(), field(), field()), 0..12)
    ) {
        let template = dialect(seed);
        prop_assert_eq!(template.skip_lines, 0);
        let records: Vec<_> = rows.iter().map(|(a, b, c)| {
            let values = [a, b, c].map(|text| representable(text, template.charset));
            Record::new(&template, &values.each_ref().map(String::as_str)).unwrap()
        }).collect();
        let bytes = write(&records, &template).unwrap();
        let parsed = read(&bytes, &template).unwrap();
        prop_assert_eq!(&parsed.records, &records);
        prop_assert_eq!(write(&parsed.records, &template).unwrap(), bytes);
    }

    #[test]
    fn arbitrary_input_bytes_never_panic(seed in any::<u16>(), bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
        let mut template = dialect(seed);
        template.skip_lines = u32::from(seed % 5);
        template.max_file_bytes = 1024;
        // A panic fails this property whether parsing returns Ok or a typed error.
        let _result = read(&bytes, &template);
    }

    #[test]
    fn disabled_quoting_roundtrips_safe_fields(
        seed in any::<u16>(), a in field(), b in field(), c in field()
    ) {
        let mut template = dialect(seed);
        template.quote = None;
        let delimiter = template.delimiter.chars().next().unwrap();
        let values = [a, b, c].map(|text| {
            representable(&text, template.charset).chars()
                .filter(|ch| !matches!(ch, '\r' | '\n' | '\u{feff}') && *ch != delimiter)
                .collect::<String>()
        });
        let record = Record::new(&template, &values.each_ref().map(String::as_str)).unwrap();
        let bytes = write(std::slice::from_ref(&record), &template).unwrap();
        prop_assert_eq!(read(&bytes, &template).unwrap().records, [record]);
    }

    #[test]
    fn typed_fields_roundtrip_without_normalizing_lexical_values(
        number in 0_u64..1_000_000_000, fraction in 0_u32..1_000_000,
        zeros in 0_usize..20, negative in any::<bool>(), year in 2000_u16..2100,
        month in 1_u8..13, day in 1_u8..29, hour in 0_u8..24,
        minute in 0_u8..60, second in 0_u8..60, enabled in any::<bool>()
    ) {
        let template = typed_template();
        let sign = if negative { "-" } else { "+" };
        let integer = format!("{sign}{}{number}", "0".repeat(zeros));
        let decimal = format!("{integer}.{fraction:06}00");
        let date = format!("{year:04}-{month:02}-{day:02}");
        let time = format!("{hour:02}:{minute:02}:{second:02}");
        let datetime = format!("{date} {time}");
        let flag = if enabled { "YES" } else { "NO" };
        let record = Record::new(&template, &[&integer, &decimal, &date, &time, &datetime, flag]).unwrap();
        let bytes = write(std::slice::from_ref(&record), &template).unwrap();
        prop_assert_eq!(read(&bytes, &template).unwrap().records, [record]);
    }
}

fn typed_template() -> Template {
    let mut columns = vec![
        column("integer", ColumnType::Integer),
        column("decimal", ColumnType::Decimal),
    ];
    for (name, kind, format) in [
        ("date", ColumnType::Date, "yyyy-MM-dd"),
        ("time", ColumnType::Time, "HH:mm:ss"),
        ("datetime", ColumnType::Datetime, "yyyy-MM-dd HH:mm:ss"),
    ] {
        columns.push(temporal(name, kind, format));
    }
    columns.push(column("enabled", boolean("YES", "NO")));
    with_columns(columns)
}
