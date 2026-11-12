use crate::{Column, ColumnType};

#[derive(Clone, Copy)]
enum Part {
    Number(usize, usize),
    Literal(char),
}
fn parts(format: &str) -> Option<Vec<Part>> {
    let mut rest = format;
    let mut result = Vec::new();
    while !rest.is_empty() {
        let token = [
            ("yyyy", 0),
            ("yy", 0),
            ("MM", 1),
            ("dd", 2),
            ("HH", 3),
            ("mm", 4),
            ("ss", 5),
        ]
        .into_iter()
        .find(|(s, _)| rest.starts_with(s));
        if let Some((token, index)) = token {
            result.push(Part::Number(index, token.len()));
            rest = &rest[token.len()..];
        } else {
            let ch = rest.chars().next()?;
            result.push(Part::Literal(ch));
            rest = &rest[ch.len_utf8()..];
        }
    }
    Some(result)
}
pub(crate) fn valid_format(column: &Column) -> bool {
    let Some(parts) = column.format.as_deref().and_then(parts) else {
        return false;
    };
    let mut counts = [0; 6];
    for p in parts {
        if let Part::Number(i, _) = p {
            counts[i] += 1;
        }
    }
    if counts.iter().any(|n| *n > 1) {
        return false;
    }
    match column.column_type {
        ColumnType::Date => counts == [1, 1, 1, 0, 0, 0],
        ColumnType::Time => counts[..3] == [0, 0, 0] && counts[3..5] == [1, 1],
        ColumnType::Datetime => counts[..5] == [1, 1, 1, 1, 1],
        _ => false,
    }
}
pub(crate) fn valid_value(column: &Column, text: &str) -> bool {
    let Some(parts) = column.format.as_deref().and_then(parts) else {
        return false;
    };
    let mut rest = text;
    let mut values = [0_u32; 6];
    for part in parts {
        match part {
            Part::Literal(ch) => {
                let Some(tail) = rest.strip_prefix(ch) else {
                    return false;
                };
                rest = tail;
            }
            Part::Number(i, n) => {
                let Some(digits) = rest.get(..n) else {
                    return false;
                };
                if !digits.bytes().all(|b| b.is_ascii_digit()) {
                    return false;
                }
                let Ok(value) = digits.parse::<u32>() else {
                    return false;
                };
                values[i] = if i == 0 && n == 2 {
                    value + 2000
                } else {
                    value
                };
                rest = &rest[n..];
            }
        }
    }
    if !rest.is_empty() || values[3] > 23 || values[4] > 59 || values[5] > 59 {
        return false;
    }
    if column.column_type == ColumnType::Time {
        return true;
    }
    let [year, month, day, ..] = values;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year != 0 && (1..=days).contains(&day)
}
