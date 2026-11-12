mod support;

use packie_files::{ColumnType, Error, PathError, Record, Template, read, write};
use support::{boolean, column, strings, temporal, with_columns};

fn analyzer_template() -> Template {
    let mut template = strings(&["control_id", "sample_id", "test", "result", "replicate"]);
    template.columns[3].column_type = ColumnType::Decimal;
    template.columns[4].column_type = ColumnType::Integer;
    for (name, kind, format) in [
        ("collected_date", ColumnType::Date, "yyyy-MM-dd"),
        ("collected_time", ColumnType::Time, "HH:mm:ss"),
        ("reported_at", ColumnType::Datetime, "yyyy-MM-dd HH:mm:ss"),
    ] {
        template.columns.push(temporal(name, kind, format));
    }
    template.columns.push(column("valid", boolean("Y", "N")));
    template
}

#[test]
fn analyzer_values_preserve_exact_lexical_spelling_and_types() {
    let template = analyzer_template();
    let document = read(include_bytes!("fixtures/analyzer.csv"), &template).unwrap();
    assert_eq!(document.records.len(), 2);
    let first = &document.records[0];
    assert_eq!(first.get("col.result"), Some("0005.2300"));
    assert_eq!(first.get("col.replicate"), Some("+01"));
    assert_eq!(first.get("col.collected_date"), Some("2024-02-29"));
    assert_eq!(first.get("col.collected_time"), Some("23:59:58"));
    for (field, expected) in first.fields().iter().zip(&template.columns) {
        assert_eq!(field.value().column_type(), &expected.column_type);
    }
    assert_eq!(first.fields()[8].value().as_boolean(), Some(true));
    assert_eq!(
        document.records[1].fields()[8].value().as_boolean(),
        Some(false)
    );
    assert_eq!(first.fields()[3].value().as_boolean(), None);
    assert_eq!(
        read(&write(&document.records, &template).unwrap(), &template).unwrap(),
        document
    );
}

#[test]
fn numeric_values_never_use_float_or_machine_integer_range() {
    let template = with_columns(vec![
        column("integer", ColumnType::Integer),
        column("decimal", ColumnType::Decimal),
    ]);
    let huge = "9999999999999999999999999999999999999999999999999999999999";
    let exact = format!("-{huge}.00000000000000000000000001");
    let record = Record::new(&template, &[huge, &exact]).unwrap();
    assert_eq!(record.get("col.integer"), Some(huge));
    assert_eq!(record.get("col.decimal"), Some(exact.as_str()));
    for invalid in ["1e3", "NaN", "inf", " 1", "1 ", "+", "١"] {
        assert!(Record::new(&template, &[invalid, "1.00"]).is_err());
        assert!(Record::new(&template, &["1", invalid]).is_err());
    }
    for invalid in [".5", "1.", "1.2.3", "--1"] {
        assert!(Record::new(&template, &["1", invalid]).is_err());
    }
}

#[test]
fn temporal_validation_checks_calendar_and_clock_boundaries() {
    for (kind, format, valid, invalid) in [
        (ColumnType::Date, "yyyy-MM-dd", "2000-02-29", "1900-02-29"),
        (ColumnType::Date, "dd/MM/yy", "29/02/24", "31/04/24"),
        (ColumnType::Time, "HH:mm:ss", "23:59:59", "24:00:00"),
        (ColumnType::Time, "HHmmss", "000000", "006000"),
        (
            ColumnType::Datetime,
            "yyyyMMddHHmmss",
            "20240229235959",
            "20230229235959",
        ),
    ] {
        let template = with_columns(vec![temporal("value", kind, format)]);
        assert!(Record::new(&template, &[valid]).is_ok(), "{valid}");
        assert!(Record::new(&template, &[invalid]).is_err(), "{invalid}");
    }
}

#[test]
fn optional_empty_values_and_required_targets_validate_at_write_time() {
    let template = analyzer_template();
    let empty = Record::new(&template, &[""; 9]).unwrap();
    assert_eq!(empty.fields()[8].value().as_boolean(), None);
    let mut required = template;
    required.columns[0].required = true;
    assert!(Record::new(&required, &[""; 9]).is_err());
    let mut target = Record::empty(&required).unwrap();
    assert!(
        matches!(write(std::slice::from_ref(&target), &required), Err(Error::InvalidValue { column, .. }) if column == "control_id")
    );
    target.set("col.control_id", "SYN-001").unwrap();
    assert!(write(&[target], &required).is_ok());
    assert!(
        matches!(read(b"control_id,sample_id,test,result,replicate,collected_date,collected_time,reported_at,valid\n,,,,,,,,\n", &required), Err(Error::InvalidValue { line: 2, column }) if column == "control_id")
    );
}

#[test]
fn paths_follow_output_order_and_failed_mutations_are_atomic() {
    let mut template = strings(&["amount", "sample"]);
    template.columns[0].position = Some(3);
    template.columns[0].column_type = ColumnType::Decimal;
    template.columns[1].position = Some(1);
    let mut record = Record::new(&template, &["001.2300", "SYN-001"]).unwrap();
    assert_eq!(record.get("col[0]"), Some("SYN-001"));
    assert_eq!(record.get("col[1]"), Some("001.2300"));
    let before = record.clone();
    assert_eq!(
        record.set("col.amount", "NaN"),
        Err(PathError::InvalidValue {
            column: "amount".into()
        })
    );
    assert_eq!(record, before);
    for path in [
        "amount",
        "col.Amount",
        "col[2]",
        "col[-1]",
        "col[]",
        "col[1].x",
        "col[999999999999999999999999999]",
    ] {
        assert_eq!(record.get(path), None);
        assert_eq!(record.set(path, "x"), Err(PathError::UnknownPath));
        assert_eq!(record, before);
    }
    record.set("col[1]", "-0.000").unwrap();
    assert_eq!(record.get("col.amount"), Some("-0.000"));
}

#[test]
fn boolean_literals_are_exact_and_case_sensitive() {
    let template = with_columns(vec![column("valid", boolean("Y", "N"))]);
    for value in ["true", "false", "y", "n", " Y", "1"] {
        assert!(Record::new(&template, &[value]).is_err());
    }
}
