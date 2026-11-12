mod support;

use packie_files::{Charset, ColumnType, Error, RecordMode, Template, read, write};
use support::{boolean, strings, temporal, with_columns};

#[test]
fn invalid_templates_collect_all_problems_in_stable_order() {
    let mut template = strings(&["invalid_name", "invalid_name"]);
    template.delimiter = ":".into();
    template.quote = Some("long".into());
    template.skip_lines = 1001;
    template.max_file_bytes = 1023;
    template.charset = Charset::Ascii;
    template.write_bom = true;
    template.file_name = Some("../out.csv".into());
    template.columns[0].name = "INVALID".into();
    template.columns[1].name = "INVALID".into();
    template.columns[0].position = Some(0);
    template.columns[1].position = Some(0);
    template.columns[0].column_type = ColumnType::Date;
    template.columns[0].format = Some("yyyy-MM".into());
    template.columns[1].column_type = boolean("", "");
    template.record_mode = RecordMode::Grouped;
    template.group_by = Some("missing".into());
    let paths = template
        .validate()
        .into_iter()
        .map(|problem| problem.path)
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "delimiter",
            "quote",
            "skip_lines",
            "max_file_bytes",
            "write_bom",
            "file_name",
            "columns[0].name",
            "columns[0].position",
            "columns[0].format",
            "columns[1].name",
            "columns[1].name",
            "columns[1].position",
            "columns[1].position",
            "columns[1].header",
            "columns[1].type",
            "group_by",
        ]
    );
    assert!(matches!(read(b"", &template), Err(Error::Template(_))));
    assert!(matches!(write(&[], &template), Err(Error::Template(_))));
}

#[test]
fn empty_columns_and_invalid_quote_and_limit_boundaries_are_rejected() {
    assert_eq!(Template::default().validate()[0].path, "columns");
    for quote in ["", "\"\"", ",", "\n", "\r", "\0"] {
        let mut template = strings(&["value"]);
        template.quote = Some(quote.into());
        assert_eq!(template.validate()[0].path, "quote", "{quote:?}");
    }
    for limit in [0, 1023, 1_073_741_825] {
        let mut template = strings(&["value"]);
        template.max_file_bytes = limit;
        assert_eq!(template.validate()[0].path, "max_file_bytes");
    }
    for limit in [1024, 1_073_741_824] {
        let mut template = strings(&["value"]);
        template.max_file_bytes = limit;
        assert!(template.validate().is_empty());
    }
}

#[test]
fn invalid_temporal_formats_and_grouping_contracts_fail_validation() {
    for format in ["", "yyyy-MM", "yyyy-MM-dd-yyyy", "yyyy-MM-ddTHH:mm:ss"] {
        let template = with_columns(vec![temporal("date", ColumnType::Date, format)]);
        assert_eq!(template.validate()[0].path, "columns[0].format");
    }
    for (mode, group) in [
        (RecordMode::Grouped, None),
        (RecordMode::Grouped, Some("missing")),
        (RecordMode::OnePerLine, Some("id")),
        (RecordMode::File, Some("id")),
    ] {
        let mut template = strings(&["id"]);
        template.record_mode = mode;
        template.group_by = group.map(str::to_owned);
        assert_eq!(template.validate()[0].path, "group_by");
    }
}
