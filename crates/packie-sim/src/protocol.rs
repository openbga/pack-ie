//! Protocol-aware `send_message` and `expect_message` (scenarios.md §7.1): HL7 v2 over
//! MLLP and ASTM E1381, written against the connection's read buffer so partial reads and
//! timeouts behave like every other step. The wire formats are implemented here, not
//! taken from the engine's crates, so an engine bug cannot hide behind a shared codec.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use regex::Regex;
use tokio::time::Instant;

use crate::actors::conn::{Conn, ConnState, Waited};
use crate::payload::preview;
use crate::steps::{Codes, ExpectMessageArgs, MessageAssert, Protocol, SendMessageArgs};

const VT: u8 = 0x0B;
const FS: u8 = 0x1C;
const CR: u8 = 0x0D;
const LF: u8 = 0x0A;
const STX: u8 = 0x02;
const ETX: u8 = 0x03;
const EOT: u8 = 0x04;
const ENQ: u8 = 0x05;
const ACK: u8 = 0x06;
const NAK: u8 = 0x15;
const ETB: u8 = 0x17;
/// Longest text of one ASTM frame.
const ASTM_FRAME_TEXT: usize = 240;
/// Retransmissions of one frame before the sender gives up.
const ASTM_RETRIES: u32 = 6;

/// Runs `send_message`.
pub async fn send_message(
    conn: &Conn,
    args: &SendMessageArgs,
    wait: Duration,
) -> Result<(), String> {
    let reply_wait = args.reply_timeout_ms.map_or(wait, Duration::from_millis);
    match args.protocol {
        Protocol::Hl7Mllp => {
            let segments = args.segments.as_deref().unwrap_or_default();
            hl7_send(conn, segments, args.expect_reply.as_ref(), reply_wait).await
        }
        Protocol::Astm => {
            let records = args.records.as_deref().unwrap_or_default();
            let (frames, omit_eot) = args
                .partial
                .as_ref()
                .map_or((None, false), |p| (p.frames, p.omit_eot));
            astm_send(conn, records, frames, omit_eot, reply_wait).await
        }
    }
}

/// Runs `expect_message`.
pub async fn expect_message(
    conn: &Conn,
    args: &ExpectMessageArgs,
    wait: Duration,
) -> Result<(), String> {
    match args.protocol {
        Protocol::Hl7Mllp => hl7_expect(conn, args, wait).await,
        Protocol::Astm => astm_expect(conn, args, wait).await,
    }
}

// ---------------------------------------------------------------------------- HL7

fn mllp_frame(message: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(message.len() + 3);
    out.push(VT);
    out.extend_from_slice(message.as_bytes());
    out.extend_from_slice(&[FS, CR]);
    out
}

/// Reads one MLLP frame; bytes before its start byte are discarded.
async fn read_mllp(conn: &Conn, wait: Duration) -> Result<String, String> {
    let deadline = Instant::now() + wait;
    let found = conn
        .wait_until(deadline, |data, state| {
            if let Some(start) = data.iter().position(|b| *b == VT)
                && let Some(end) = data[start..].windows(2).position(|w| w == [FS, CR])
            {
                let body = data[start + 1..start + end].to_vec();
                return Some(Ok((body, start + end + 2)));
            }
            (!state.is_open()).then(|| Err(state.clone()))
        })
        .await;
    match found {
        Waited::Ready(Ok((body, consumed))) => {
            let _ = conn.take(consumed);
            Ok(String::from_utf8_lossy(&body).into_owned())
        }
        Waited::Ready(Err(state)) => Err(format!(
            "connection {state} before a complete MLLP frame arrived (buffered: {})",
            preview(&conn.peek(120), 120)
        )),
        Waited::TimedOut => Err(format!(
            "no complete MLLP frame within {} ms (buffered {} bytes: {})",
            wait.as_millis(),
            conn.buffered(),
            preview(&conn.peek(120), 120)
        )),
    }
}

fn hl7_segments(message: &str) -> Vec<&str> {
    message
        .split(['\r', '\n'])
        .filter(|s| !s.is_empty())
        .collect()
}

/// The fields of a segment: split on the field separator of the message's MSH.
fn hl7_fields(segment: &str, sep: char) -> Vec<&str> {
    segment.split(sep).collect()
}

fn hl7_separators(message: &str) -> (char, char, char, char) {
    let msh = message.trim_start_matches(['\r', '\n']);
    let mut chars = msh.chars().skip(3);
    let field = chars.next().unwrap_or('|');
    let encoding: Vec<char> = msh.chars().skip(4).take(4).collect();
    let get = |i: usize, default: char| encoding.get(i).copied().unwrap_or(default);
    (field, get(0, '^'), get(1, '~'), get(3, '&'))
}

/// MSH-10 of a message that starts with an MSH segment.
fn msh_control_id(message: &str) -> Option<String> {
    let first = hl7_segments(message).into_iter().next()?;
    if !first.starts_with("MSH") {
        return None;
    }
    let (sep, ..) = hl7_separators(first);
    hl7_fields(first, sep)
        .get(9)
        .map(|s| (*s).to_owned())
        .filter(|s| !s.is_empty())
}

async fn hl7_send(
    conn: &Conn,
    segments: &[String],
    expect: Option<&Codes>,
    wait: Duration,
) -> Result<(), String> {
    let mut message = segments.join("\r");
    message.push('\r');
    conn.write(&mllp_frame(&message), None, Duration::ZERO)
        .await
        .map_err(|e| format!("write failed: {e}"))?;
    let codes = match expect {
        None => vec!["AA".to_owned()],
        Some(Codes::None) => return Ok(()),
        Some(Codes::List(list)) => list.clone(),
    };
    let sent_id = msh_control_id(&message);
    for (i, code) in codes.iter().enumerate() {
        let reply = read_mllp(conn, wait)
            .await
            .map_err(|e| format!("reply {} of {} ({code}): {e}", i + 1, codes.len()))?;
        let msa = hl7_segments(&reply)
            .into_iter()
            .find(|s| s.starts_with("MSA"))
            .ok_or_else(|| {
                format!(
                    "reply {} has no MSA segment: {}",
                    i + 1,
                    preview(reply.as_bytes(), 160)
                )
            })?;
        let (sep, ..) = hl7_separators(&reply);
        let fields = hl7_fields(msa, sep);
        let observed = fields.get(1).copied().unwrap_or_default();
        if observed != code {
            return Err(format!(
                "reply {}: expected MSA-1 {code}, observed {observed:?} ({})",
                i + 1,
                preview(msa.as_bytes(), 160)
            ));
        }
        if let Some(id) = &sent_id {
            let echoed = fields.get(2).copied().unwrap_or_default();
            if echoed != id {
                return Err(format!(
                    "reply {}: expected MSA-2 {id:?} (the sent MSH-10), observed {echoed:?}",
                    i + 1
                ));
            }
        }
    }
    Ok(())
}

async fn hl7_expect(conn: &Conn, args: &ExpectMessageArgs, wait: Duration) -> Result<(), String> {
    let message = read_mllp(conn, wait).await?;
    check_asserts(&args.assert, |path| hl7_path(&message, path))
        .map_err(|e| format!("{e}\n    message: {}", preview(message.as_bytes(), 400)))?;
    let codes = match &args.reply {
        None => vec!["AA".to_owned()],
        Some(Codes::None) => return Ok(()),
        Some(Codes::List(list)) => list.clone(),
    };
    for (i, code) in codes.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(Duration::from_millis(args.reply_delay_ms.unwrap_or(0))).await;
        }
        let ack = build_ack(&message, code, i);
        conn.write(&mllp_frame(&ack), None, Duration::ZERO)
            .await
            .map_err(|e| format!("cannot send ACK {code}: {e}"))?;
    }
    Ok(())
}

fn build_ack(message: &str, code: &str, n: usize) -> String {
    let first = hl7_segments(message).into_iter().next().unwrap_or_default();
    let (sep, ..) = hl7_separators(first);
    let f = hl7_fields(first, sep);
    let at = |i: usize| f.get(i).copied().unwrap_or_default();
    let event = at(8).split('^').nth(1).unwrap_or_default();
    let control = at(9);
    let ack_type = if event.is_empty() {
        "ACK".to_owned()
    } else {
        format!("ACK^{event}")
    };
    format!(
        "MSH{sep}^~\\&{sep}{}{sep}{}{sep}{}{sep}{}{sep}{}{sep}{sep}{ack_type}{sep}SIM-ACK-{control}-{n}{sep}{}{sep}{}\rMSA{sep}{code}{sep}{control}\r",
        at(4),
        at(5),
        at(2),
        at(3),
        hl7_timestamp(SystemTime::now()),
        at(10),
        at(11),
    )
}

/// `YYYYMMDDHHMMSS` in UTC.
fn hl7_timestamp(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    let z = i64::try_from(days).unwrap_or(0) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

/// A parsed message path: `SEG[occ].field[rep].component.subcomponent`.
struct HlPath<'a> {
    segment: &'a str,
    occurrence: usize,
    /// Field, component and subcomponent numbers (from 1).
    numbers: Vec<usize>,
    /// The `[rep]` after the field, when given.
    repetition: Option<usize>,
}

fn parse_hl_path(path: &str) -> Option<HlPath<'_>> {
    let mut parts = path.split('.');
    let (segment, occurrence) = split_index(parts.next()?)?;
    let mut numbers = Vec::new();
    let mut repetition = None;
    for (i, part) in parts.enumerate() {
        let (number, index) = split_index(part)?;
        numbers.push(number.parse().ok()?);
        if i == 0 && part.contains('[') {
            repetition = Some(index);
        }
    }
    Some(HlPath {
        segment,
        occurrence,
        numbers,
        repetition,
    })
}

/// `name[3]` into (`name`, 3); no bracket gives index 0.
fn split_index(part: &str) -> Option<(&str, usize)> {
    match part.split_once('[') {
        None => Some((part, 0)),
        Some((name, rest)) => Some((name, rest.strip_suffix(']')?.parse().ok()?)),
    }
}

/// The text at an HL7 path; `None` when the segment is absent, empty text when only the
/// field is.
fn hl7_path(message: &str, path: &str) -> Option<String> {
    let parsed = parse_hl_path(path)?;
    let segments = hl7_segments(message);
    let (field_sep, comp, rep, sub) = hl7_separators(segments.first()?);
    let segment = segments
        .iter()
        .filter(|s| s.split(field_sep).next() == Some(parsed.segment))
        .nth(parsed.occurrence)?;
    let fields = hl7_fields(segment, field_sep);
    let number = *parsed.numbers.first()?;
    let field = match (parsed.segment, number) {
        ("MSH", 1) => field_sep.to_string(),
        ("MSH", n) => fields.get(n - 1).copied().unwrap_or_default().to_owned(),
        _ => fields.get(number).copied().unwrap_or_default().to_owned(),
    };
    if parsed.numbers.len() == 1 && parsed.repetition.is_none() {
        return Some(field);
    }
    let repetition = field
        .split(rep)
        .nth(parsed.repetition.unwrap_or(0))
        .unwrap_or_default();
    let Some(&component) = parsed.numbers.get(1) else {
        return Some(repetition.to_owned());
    };
    let component_text = repetition
        .split(comp)
        .nth(component.checked_sub(1)?)
        .unwrap_or_default();
    match parsed.numbers.get(2) {
        None => Some(component_text.to_owned()),
        Some(&s) => Some(
            component_text
                .split(sub)
                .nth(s.checked_sub(1)?)
                .unwrap_or_default()
                .to_owned(),
        ),
    }
}

fn presence(present: bool) -> &'static str {
    if present { "present" } else { "absent" }
}

fn check_asserts(
    asserts: &[MessageAssert],
    lookup: impl Fn(&str) -> Option<String>,
) -> Result<(), String> {
    for entry in asserts {
        let value = lookup(&entry.path);
        let text = value.clone().unwrap_or_default();
        let ok = match (&entry.equals, &entry.matches, entry.exists) {
            (Some(want), _, _) => &text == want,
            (_, Some(pattern), _) => Regex::new(pattern).is_ok_and(|re| re.is_match(&text)),
            (_, _, Some(want)) => !text.is_empty() == want,
            _ => true,
        };
        if !ok {
            let want = entry
                .equals
                .as_ref()
                .map(|w| format!("equal to {w:?}"))
                .or_else(|| entry.matches.as_ref().map(|m| format!("matching /{m}/")))
                .or_else(|| entry.exists.map(|e| presence(e).to_owned()))
                .unwrap_or_default();
            let observed = match value {
                Some(v) => format!("{v:?}"),
                None => "<path not found>".to_owned(),
            };
            return Err(format!(
                "assert {}: expected {want}, observed {observed}",
                entry.path
            ));
        }
    }
    Ok(())
}

// --------------------------------------------------------------------------- ASTM

fn astm_checksum(bytes: &[u8]) -> String {
    format!(
        "{:02X}",
        bytes.iter().fold(0_u8, |sum, b| sum.wrapping_add(*b))
    )
}

/// `STX FN text ETX|ETB C1 C2 CR LF`.
fn astm_frame(number: u8, text: &[u8], last: bool) -> Vec<u8> {
    let mut body = vec![b'0' + number];
    body.extend_from_slice(text);
    body.push(if last { ETX } else { ETB });
    let mut frame = vec![STX];
    frame.extend_from_slice(&body);
    frame.extend_from_slice(astm_checksum(&body).as_bytes());
    frame.extend_from_slice(&[CR, LF]);
    frame
}

async fn read_byte(conn: &Conn, wait: Duration) -> Result<u8, String> {
    let found = conn
        .wait_until(Instant::now() + wait, |data, state| {
            data.first()
                .map(|b| Ok(*b))
                .or_else(|| (!state.is_open()).then(|| Err(state.clone())))
        })
        .await;
    match found {
        Waited::Ready(Ok(byte)) => {
            let _ = conn.take(1);
            Ok(byte)
        }
        Waited::Ready(Err(state)) => {
            Err(format!("connection {state} while waiting for a reply byte"))
        }
        Waited::TimedOut => Err(format!("no reply byte within {} ms", wait.as_millis())),
    }
}

fn control_name(byte: u8) -> String {
    match byte {
        ENQ => "ENQ".into(),
        ACK => "ACK".into(),
        NAK => "NAK".into(),
        EOT => "EOT".into(),
        other => format!("0x{other:02x}"),
    }
}

async fn astm_send(
    conn: &Conn,
    records: &[String],
    stop_after_frames: Option<u32>,
    omit_eot: bool,
    wait: Duration,
) -> Result<(), String> {
    let send = |bytes: Vec<u8>| async move {
        conn.write(&bytes, None, Duration::ZERO)
            .await
            .map_err(|e| format!("write failed: {e}"))
    };
    send(vec![ENQ]).await?;
    match read_byte(conn, wait)
        .await
        .map_err(|e| format!("after ENQ: {e}"))?
    {
        ACK => {}
        other => {
            return Err(format!(
                "after ENQ the receiver answered {} instead of ACK",
                control_name(other)
            ));
        }
    }
    let mut frames_sent = 0_u32;
    for (index, record) in records.iter().enumerate() {
        let mut text = record.as_bytes().to_vec();
        text.push(CR);
        let chunks: Vec<&[u8]> = text.chunks(ASTM_FRAME_TEXT).collect();
        for (c, chunk) in chunks.iter().enumerate() {
            if stop_after_frames.is_some_and(|n| frames_sent >= n) {
                return Ok(());
            }
            let number = u8::try_from((frames_sent + 1) % 8).unwrap_or(1);
            let frame = astm_frame(number, chunk, c + 1 == chunks.len());
            astm_deliver(conn, &frame, wait, index).await?;
            frames_sent += 1;
        }
    }
    if stop_after_frames.is_some() || omit_eot {
        return Ok(());
    }
    send(vec![EOT]).await
}

/// Sends one frame, retransmitting after NAK.
async fn astm_deliver(
    conn: &Conn,
    frame: &[u8],
    wait: Duration,
    record: usize,
) -> Result<(), String> {
    for attempt in 0..=ASTM_RETRIES {
        conn.write(frame, None, Duration::ZERO)
            .await
            .map_err(|e| format!("write failed: {e}"))?;
        match read_byte(conn, wait)
            .await
            .map_err(|e| format!("record {record}, attempt {}: {e}", attempt + 1))?
        {
            ACK => return Ok(()),
            NAK => {}
            other => {
                return Err(format!(
                    "record {record}: the receiver answered {} instead of ACK or NAK",
                    control_name(other)
                ));
            }
        }
    }
    Err(format!(
        "record {record}: NAK after {ASTM_RETRIES} retransmissions"
    ))
}

enum AstmItem {
    Enq,
    Eot,
    Frame(Vec<u8>),
    Other(u8),
    Closed(ConnState),
}

async fn read_astm_item(conn: &Conn, wait: Duration) -> Result<AstmItem, String> {
    let found = conn
        .wait_until(Instant::now() + wait, |data, state| match data.first() {
            Some(&ENQ) => Some((AstmItem::Enq, 1)),
            Some(&EOT) => Some((AstmItem::Eot, 1)),
            Some(&STX) => data
                .iter()
                .position(|b| *b == LF)
                .map(|end| (AstmItem::Frame(data[..=end].to_vec()), end + 1)),
            Some(&other) => Some((AstmItem::Other(other), 1)),
            None => (!state.is_open()).then(|| (AstmItem::Closed(state.clone()), 0)),
        })
        .await;
    match found {
        Waited::Ready((item, consumed)) => {
            let _ = conn.take(consumed);
            Ok(item)
        }
        Waited::TimedOut => Err(format!(
            "nothing complete arrived within {} ms (buffered {} bytes: {})",
            wait.as_millis(),
            conn.buffered(),
            preview(&conn.peek(120), 120)
        )),
    }
}

/// Validates a frame: `(frame number, text, is_last)`.
fn parse_astm_frame(frame: &[u8]) -> Result<(u8, Vec<u8>, bool), String> {
    let end = frame
        .iter()
        .position(|b| *b == ETX || *b == ETB)
        .ok_or("frame has no ETX or ETB")?;
    let body = &frame[1..=end];
    let sent = frame
        .get(end + 1..end + 3)
        .ok_or("frame is too short for a checksum")?;
    let expected = astm_checksum(body);
    if sent != expected.as_bytes() {
        return Err(format!(
            "checksum {} does not match computed {expected}",
            String::from_utf8_lossy(sent)
        ));
    }
    let number = body.first().copied().ok_or("frame has no number")?;
    Ok((number, body[1..body.len() - 1].to_vec(), frame[end] == ETX))
}

async fn astm_expect(conn: &Conn, args: &ExpectMessageArgs, wait: Duration) -> Result<(), String> {
    match read_astm_item(conn, wait).await? {
        AstmItem::Enq => {}
        AstmItem::Closed(state) => return Err(format!("connection {state} before ENQ")),
        AstmItem::Other(b) => return Err(format!("expected ENQ, observed {}", control_name(b))),
        AstmItem::Eot => return Err("expected ENQ, observed EOT".into()),
        AstmItem::Frame(_) => return Err("expected ENQ, observed a frame".into()),
    }
    if matches!(&args.reply, Some(Codes::None)) {
        return Ok(());
    }
    let reply = |byte: u8| async move {
        conn.write(&[byte], None, Duration::ZERO)
            .await
            .map_err(|e| format!("write failed: {e}"))
    };
    reply(ACK).await?;
    let records = astm_receive_frames(conn, args.nak_frames.unwrap_or(0), wait, &reply).await?;
    check_asserts(&args.assert, |path| astm_path(&records, path)).map_err(|e| {
        format!(
            "{e}\n    records: {}",
            preview(records.join("\r").as_bytes(), 400)
        )
    })
}

async fn astm_receive_frames<F, Fut>(
    conn: &Conn,
    nak_frames: u32,
    wait: Duration,
    reply: &F,
) -> Result<Vec<String>, String>
where
    F: Fn(u8) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let mut text = Vec::new();
    let mut naked = 0_u32;
    let mut distinct = 0_u32;
    let mut last_number: Option<u8> = None;
    loop {
        match read_astm_item(conn, wait).await? {
            AstmItem::Eot => break,
            AstmItem::Frame(frame) => match parse_astm_frame(&frame) {
                Err(_) => reply(NAK).await?,
                Ok((number, part, _)) => {
                    let retransmission = last_number == Some(number);
                    if !retransmission && naked < nak_frames && distinct == naked {
                        naked += 1;
                        reply(NAK).await?;
                        continue;
                    }
                    if !retransmission {
                        text.extend_from_slice(&part);
                        distinct += 1;
                        last_number = Some(number);
                    }
                    reply(ACK).await?;
                }
            },
            AstmItem::Closed(state) => return Err(format!("connection {state} before EOT")),
            AstmItem::Other(b) => {
                return Err(format!(
                    "unexpected byte {} during the transfer",
                    control_name(b)
                ));
            }
            AstmItem::Enq => return Err("unexpected ENQ during the transfer".into()),
        }
    }
    Ok(String::from_utf8_lossy(&text)
        .split('\r')
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .collect())
}

/// The text at an ASTM path `R[occ].field`; fields count from 1, the record type is field 1.
fn astm_path(records: &[String], path: &str) -> Option<String> {
    let (record, field) = path.split_once('.')?;
    let (kind, occurrence) = split_index(record)?;
    let number: usize = field.parse().ok()?;
    let found = records
        .iter()
        .filter(|r| r.split('|').next() == Some(kind))
        .nth(occurrence)?;
    Some(
        found
            .split('|')
            .nth(number.checked_sub(1)?)
            .unwrap_or_default()
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORU: &str = "MSH|^~\\&|SIMLAB|SIMFAC|PACKIE|PACKIEFAC|20260101120000||ORU^R01|CTRL-1|P|2.5.1\rPID|1||PID-TEST-002~ALT^x||TEST^PATIENT\rOBX|1|NM|GLU||95\rOBX|2|NM|K||4.1\r";

    #[test]
    fn hl7_paths() {
        assert_eq!(hl7_path(ORU, "MSH.10").as_deref(), Some("CTRL-1"));
        assert_eq!(hl7_path(ORU, "MSH.9").as_deref(), Some("ORU^R01"));
        assert_eq!(hl7_path(ORU, "MSH.9.2").as_deref(), Some("R01"));
        assert_eq!(
            hl7_path(ORU, "PID.3").as_deref(),
            Some("PID-TEST-002~ALT^x")
        );
        assert_eq!(hl7_path(ORU, "PID.3[0]").as_deref(), Some("PID-TEST-002"));
        assert_eq!(hl7_path(ORU, "PID.3[1].1").as_deref(), Some("ALT"));
        assert_eq!(hl7_path(ORU, "OBX.5").as_deref(), Some("95"));
        assert_eq!(hl7_path(ORU, "OBX[1].5").as_deref(), Some("4.1"));
        assert_eq!(hl7_path(ORU, "ZZZ.1"), None);
    }

    #[test]
    fn ack_echoes_the_control_id_and_swaps_parties() {
        let ack = build_ack(ORU, "AA", 0);
        assert!(
            ack.starts_with("MSH|^~\\&|PACKIE|PACKIEFAC|SIMLAB|SIMFAC|"),
            "{ack}"
        );
        assert!(ack.contains("|ACK^R01|"), "{ack}");
        assert!(ack.ends_with("MSA|AA|CTRL-1\r"), "{ack}");
        assert_eq!(msh_control_id(&ack).as_deref(), Some("SIM-ACK-CTRL-1-0"));
    }

    #[test]
    fn timestamps_are_calendar_correct() {
        let t = UNIX_EPOCH + Duration::from_secs(1_767_268_800); // 2026-01-01T12:00:00Z
        assert_eq!(hl7_timestamp(t), "20260101120000");
        assert_eq!(
            hl7_timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "20000229000000"
        );
    }

    #[test]
    fn astm_frames_carry_number_and_checksum() {
        let frame = astm_frame(1, b"H|\\^&|\r", true);
        assert_eq!(frame[0], STX);
        let (number, text, last) = parse_astm_frame(&frame).unwrap();
        assert_eq!(
            (number, text.as_slice(), last),
            (b'1', &b"H|\\^&|\r"[..], true)
        );
        let mut bad = frame;
        bad[2] = b'X';
        assert!(parse_astm_frame(&bad).is_err());
    }

    #[test]
    fn astm_paths_count_fields_from_one() {
        let records = vec![
            "H|\\^&|".to_owned(),
            "O|1|SID-0001||^^^GLU".to_owned(),
            "R|1|^^^GLU|95|mg/dL".to_owned(),
        ];
        assert_eq!(astm_path(&records, "R.4").as_deref(), Some("95"));
        assert_eq!(astm_path(&records, "O.3").as_deref(), Some("SID-0001"));
        assert_eq!(astm_path(&records, "R[1].3"), None);
    }
}
