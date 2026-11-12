use crate::fields::validate_header;
use crate::frame::check_text;
use crate::{
    AuConfig, AuError, BlockId, Demographics, Frame, FrameEnd, Message, Query, Redacted,
    ResultBlock, ResultKind, ResultTest, SampleHeader, SessionKind, TriBlock, TriTest,
};

/// Parses fixed-width text with the caller's instrument-matched settings (pp. 8-14).
pub fn parse_message(config: &AuConfig, frame: &Frame) -> Result<Message, AuError> {
    config.validate()?;
    let bytes = frame.text.expose();
    check_text(config, bytes)?;
    let mut cursor = Cursor { bytes, offset: 0 };
    let code = cursor.take(2)?;
    let unit_no = if config.format.unit_no && code[0] != b'S' {
        let unit = cursor.take(2)?;
        if !unit.iter().all(u8::is_ascii_digit) {
            return Err(AuError::Field);
        }
        Some(Redacted::new(unit.to_vec()))
    } else {
        None
    };
    if let Some(kind) = session_kind(code) {
        cursor.expect_end()?;
        require_text_end(frame.end)?;
        return Ok(Message::Session { kind, unit_no });
    }
    match code {
        b"R " | b"RH" => parse_query(config, cursor, unit_no, code == b"RH", frame.end),
        b"S " | b"SH" => parse_tri(config, cursor, code == b"SH", frame.end),
        _ => parse_result(config, cursor, unit_no, result_kind(code)?, frame.end),
    }
}

fn session_kind(code: &[u8]) -> Option<SessionKind> {
    match code {
        b"RB" => Some(SessionKind::QueryStart),
        b"RE" => Some(SessionKind::QueryEnd),
        b"DB" => Some(SessionKind::ResultsStart),
        b"DE" => Some(SessionKind::ResultsEnd),
        b"SE" => Some(SessionKind::TriStop),
        _ => None,
    }
}
fn result_kind(code: &[u8]) -> Result<ResultKind, AuError> {
    match code {
        b"D " => Ok(ResultKind::Normal),
        b"DH" => Ok(ResultKind::Rerun),
        b"d " => Ok(ResultKind::Stat),
        b"dH" => Ok(ResultKind::StatRerun),
        b"DR" => Ok(ResultKind::ReagentBlank),
        b"DA" => Ok(ResultKind::Calibration),
        b"DQ" => Ok(ResultKind::QualityControl),
        _ => Err(AuError::Classification),
    }
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Cursor<'a> {
    fn take(&mut self, width: usize) -> Result<&'a [u8], AuError> {
        let end = self.offset.checked_add(width).ok_or(AuError::Length)?;
        let field = self.bytes.get(self.offset..end).ok_or(AuError::Length)?;
        self.offset = end;
        Ok(field)
    }
    fn raw(&mut self, width: usize) -> Result<Redacted<Vec<u8>>, AuError> {
        Ok(Redacted::new(self.take(width)?.to_vec()))
    }
    fn remaining(&self) -> &'a [u8] {
        &self.bytes[self.offset..]
    }
    fn expect_end(&self) -> Result<(), AuError> {
        if self.remaining().is_empty() {
            Ok(())
        } else {
            Err(AuError::Length)
        }
    }
}

/// Session and query messages are never blocked; only a text end is valid.
fn require_text_end(end: FrameEnd) -> Result<(), AuError> {
    if end == FrameEnd::Text {
        Ok(())
    } else {
        Err(AuError::BlockTerminator)
    }
}

fn header(
    config: &AuConfig,
    cursor: &mut Cursor<'_>,
    unit_no: Option<Redacted<Vec<u8>>>,
) -> Result<SampleHeader, AuError> {
    let rack = cursor.raw(usize::from(config.format.rack_digits))?;
    let cup = cursor.raw(config.format.cup_width())?;
    Ok(SampleHeader {
        unit_no,
        rack,
        cup,
        sample_type: cursor.raw(1)?,
        sample_no: cursor.raw(4)?,
        sample_id: cursor.raw(usize::from(config.format.sample_id_digits))?,
        reserved: Redacted::new(Vec::new()),
        demographics: None,
    })
}

fn demographics(config: &AuConfig, cursor: &mut Cursor<'_>) -> Result<Demographics, AuError> {
    let [sex, age, month] = config.format.demographic_widths();
    let sex = cursor.raw(sex)?;
    let age = cursor.raw(age)?;
    let month = cursor.raw(month)?;
    let mut information = std::array::from_fn(|_| Redacted::new(Vec::new()));
    for (field, width) in information
        .iter_mut()
        .zip(config.format.patient_info_digits)
    {
        *field = cursor.raw(usize::from(width))?;
    }
    Ok(Demographics {
        sex,
        age,
        month,
        information,
    })
}

fn parse_query(
    config: &AuConfig,
    mut cursor: Cursor<'_>,
    unit: Option<Redacted<Vec<u8>>>,
    rerun: bool,
    end: FrameEnd,
) -> Result<Message, AuError> {
    let header = header(config, &mut cursor, unit)?;
    cursor.expect_end()?;
    require_text_end(end)?;
    validate_header(&header, None)?;
    Ok(Message::Query(Query { rerun, header }))
}

fn blocked_header(
    config: &AuConfig,
    cursor: &mut Cursor<'_>,
    unit: Option<Redacted<Vec<u8>>>,
    normal: bool,
    end: FrameEnd,
) -> Result<(SampleHeader, BlockId), AuError> {
    let mut header = header(config, cursor, unit)?;
    header.reserved = cursor.raw(4)?;
    let block = match cursor.take(1)? {
        [b'E'] => BlockId::Final,
        [digit @ b'0'..=b'9'] => BlockId::Intermediate(digit - b'0'),
        _ => return Err(AuError::BlockId),
    };
    // Only ETB-enabled profiles end intermediate blocks with ETB; everything else ends with text.
    let expected = if config.etb_enabled() && block != BlockId::Final {
        FrameEnd::Block
    } else {
        FrameEnd::Text
    };
    if end != expected {
        return Err(AuError::BlockTerminator);
    }
    if normal {
        header.demographics = Some(demographics(config, cursor)?);
    }
    Ok((header, block))
}

fn parse_result(
    config: &AuConfig,
    mut cursor: Cursor<'_>,
    unit: Option<Redacted<Vec<u8>>>,
    kind: ResultKind,
    end: FrameEnd,
) -> Result<Message, AuError> {
    if config.format.rack_digits == 0
        && matches!(
            kind,
            ResultKind::ReagentBlank | ResultKind::Calibration | ResultKind::QualityControl
        )
    {
        return Err(AuError::Configuration);
    }
    let (header, block) = blocked_header(config, &mut cursor, unit, kind.normal(), end)?;
    validate_header(&header, Some(kind))?;
    let width = config.format.result_item_width();
    if !cursor.remaining().len().is_multiple_of(width) {
        return Err(AuError::Length);
    }
    let mut tests = Vec::new();
    while !cursor.remaining().is_empty() {
        let code = test_code(config, &mut cursor)?;
        tests.push(ResultTest {
            code,
            value: cursor.raw(usize::from(config.format.result_digits))?,
            flags: cursor.raw(2)?,
        });
    }
    if tests.is_empty() {
        return Err(AuError::Length);
    }
    Ok(Message::ResultBlock(ResultBlock {
        kind,
        header,
        block,
        tests,
    }))
}

fn parse_tri(
    config: &AuConfig,
    mut cursor: Cursor<'_>,
    rerun: bool,
    end: FrameEnd,
) -> Result<Message, AuError> {
    let (header, block) = blocked_header(config, &mut cursor, None, !rerun, end)?;
    validate_header(
        &header,
        Some(if rerun {
            ResultKind::Rerun
        } else {
            ResultKind::Normal
        }),
    )?;
    let width = usize::from(config.format.test_code_digits) + usize::from(rerun);
    if !cursor.remaining().len().is_multiple_of(width) {
        return Err(AuError::Length);
    }
    let mut tests = Vec::new();
    while !cursor.remaining().is_empty() {
        let code = test_code(config, &mut cursor)?;
        let method = if rerun {
            let raw = cursor.take(1)?[0];
            if !matches!(raw, b'0'..=b'2') {
                return Err(AuError::Field);
            }
            Some(raw)
        } else {
            None
        };
        tests.push(TriTest { code, method });
    }
    Ok(Message::TriBlock(TriBlock {
        rerun,
        header,
        block,
        tests,
    }))
}
fn test_code(config: &AuConfig, cursor: &mut Cursor<'_>) -> Result<Redacted<Vec<u8>>, AuError> {
    let code = cursor.take(usize::from(config.format.test_code_digits))?;
    if !code.iter().all(u8::is_ascii_digit) || code.iter().all(|b| *b == b'0') {
        return Err(AuError::Field);
    }
    Ok(Redacted::new(code.to_vec()))
}
