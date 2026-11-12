use crate::{
    AuConfig, AuError, Frame, FrameEnd, Message, Redacted, SampleHeader, encode_frame,
    parse_message,
};

/// Explicit test code and rerun method supplied by the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TriTest {
    /// ASCII digits of the configured test code width, not all zeros, preserved as bytes.
    pub code: Redacted<Vec<u8>>,
    /// Rerun ASCII method byte 0/1/2; absent for a normal requisition.
    pub method: Option<u8>,
}

/// Caller-provided requisition; the codec never infers sample identity or tests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tri {
    /// Encode as `SH` (rerun) rather than `S `.
    pub rerun: bool,
    /// The complete sample header, including demographics for normal requisitions.
    pub header: SampleHeader,
    /// Requested tests; at least one.
    pub tests: Vec<TriTest>,
}

/// Encodes SE (session stop), which is not a per-sample no-order response (p. 10).
pub fn encode_stop(config: &AuConfig) -> Result<Vec<u8>, AuError> {
    encode_frame(config, b"SE", FrameEnd::Text)
}

/// Splits TRI only between complete test items, repeating all header data (pp. 7, 9).
/// S messages intentionally omit optional incoming Unit No (manual pp. 9-10).
pub fn encode_tri(config: &AuConfig, tri: &Tri) -> Result<Vec<Vec<u8>>, AuError> {
    config.validate()?;
    // The supplied manual does not qualify an empty per-sample reply.
    if tri.tests.is_empty() {
        return Err(AuError::Length);
    }
    let (prefix, block_offset) = tri_prefix(config, tri)?;
    let item_width = usize::from(config.format.test_code_digits) + usize::from(tri.rerun);
    let capacity = usize::from(config.framing.max_wire_len)
        .checked_sub(config.overhead() + prefix.len())
        .ok_or(AuError::FrameTooLong)?;
    let per_block = capacity / item_width;
    if per_block == 0 {
        return Err(AuError::FrameTooLong);
    }
    let count = tri.tests.len().div_ceil(per_block);
    if count > 11 {
        return Err(AuError::TooManyBlocks);
    }
    let mut frames = Vec::new();
    for index in 0..count {
        let final_block = index + 1 == count;
        let mut text = prefix.clone();
        text[block_offset] = if final_block {
            b'E'
        } else {
            b'0' + u8::try_from(index).map_err(|_| AuError::TooManyBlocks)?
        };
        for test in tri.tests.iter().skip(index * per_block).take(per_block) {
            append_test(config, &mut text, test, tri.rerun)?;
        }
        let end = if final_block {
            FrameEnd::Text
        } else {
            FrameEnd::Block
        };
        let wire = encode_frame(config, &text, end)?;
        // Reuse the single fixed-field validator at this outward trust boundary.
        let check = Frame {
            wire: Redacted::new(wire.clone()),
            text: Redacted::new(text),
            end: if config.etb_enabled() {
                end
            } else {
                FrameEnd::Text
            },
        };
        if !matches!(parse_message(config, &check)?, Message::TriBlock(_)) {
            return Err(AuError::Classification);
        }
        frames.push(wire);
    }
    Ok(frames)
}

fn append_field(
    text: &mut Vec<u8>,
    value: &Redacted<Vec<u8>>,
    width: usize,
) -> Result<(), AuError> {
    if value.expose().len() != width {
        return Err(AuError::Length);
    }
    text.extend_from_slice(value.expose());
    Ok(())
}

/// The text shared by every block of a requisition, and the offset of its block identification.
fn tri_prefix(config: &AuConfig, tri: &Tri) -> Result<(Vec<u8>, usize), AuError> {
    let mut text = if tri.rerun {
        b"SH".to_vec()
    } else {
        b"S ".to_vec()
    };
    let header = &tri.header;
    let format = &config.format;
    for (field, width) in [
        (&header.rack, usize::from(format.rack_digits)),
        (&header.cup, format.cup_width()),
        (&header.sample_type, 1),
        (&header.sample_no, 4),
        (&header.sample_id, usize::from(format.sample_id_digits)),
        (&header.reserved, 4),
    ] {
        append_field(&mut text, field, width)?;
    }
    let block_offset = text.len();
    text.push(b'E');
    if tri.rerun {
        if header.demographics.is_some() {
            return Err(AuError::Field);
        }
    } else {
        let demographics = header.demographics.as_ref().ok_or(AuError::Field)?;
        let [sex, age, month] = format.demographic_widths();
        let widths = [sex, age, month]
            .into_iter()
            .chain(format.patient_info_digits.map(usize::from));
        let fields = [&demographics.sex, &demographics.age, &demographics.month]
            .into_iter()
            .chain(&demographics.information);
        for (field, width) in fields.zip(widths) {
            append_field(&mut text, field, width)?;
        }
    }
    Ok((text, block_offset))
}
fn append_test(
    config: &AuConfig,
    text: &mut Vec<u8>,
    item: &TriTest,
    rerun: bool,
) -> Result<(), AuError> {
    append_field(
        text,
        &item.code,
        usize::from(config.format.test_code_digits),
    )?;
    match (rerun, item.method) {
        (true, Some(method @ b'0'..=b'2')) => text.push(method),
        (false, None) => {}
        _ => return Err(AuError::Field),
    }
    Ok(())
}
