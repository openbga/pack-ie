//! Wire formats of the soak peers: MLLP framing, deterministic HL7 and ASTM messages,
//! delimited files and the self-checking records of plain bridges.
//!
//! Every message is a pure function of its channel and sequence number, so a receiving peer
//! can verify the content it gets without keeping what was sent. Patient data is the
//! placeholder `TEST^PATIENT` with MRN `MRN-00000`.

use super::topology::Plan;

pub(crate) const VT: u8 = 0x0B;
pub(crate) const FS: u8 = 0x1C;
pub(crate) const CR: u8 = 0x0D;
pub(crate) const STX: u8 = 0x02;
pub(crate) const ETX: u8 = 0x03;
pub(crate) const EOT: u8 = 0x04;
pub(crate) const ENQ: u8 = 0x05;
pub(crate) const ACK: u8 = 0x06;
pub(crate) const NAK: u8 = 0x15;

/// `VT message FS CR`.
pub(crate) fn mllp(message: &str) -> Vec<u8> {
    let mut frame = Vec::with_capacity(message.len() + 3);
    frame.push(VT);
    frame.extend_from_slice(message.as_bytes());
    frame.extend_from_slice(&[FS, CR]);
    frame
}

/// Bytes read from a stream, cut into MLLP frames.
#[derive(Debug, Default)]
pub(crate) struct FrameBuf {
    bytes: Vec<u8>,
}

impl FrameBuf {
    pub(crate) fn push(&mut self, chunk: &[u8]) {
        self.bytes.extend_from_slice(chunk);
    }

    /// The text of the next complete frame.
    pub(crate) fn next_frame(&mut self) -> Option<String> {
        let start = self.bytes.iter().position(|b| *b == VT)?;
        let end = self.bytes[start..].windows(2).position(|w| w == [FS, CR])? + start;
        let text = String::from_utf8_lossy(&self.bytes[start + 1..end]).into_owned();
        self.bytes.drain(..end + 2);
        Some(text)
    }
}

/// The time `seq` seconds after midnight of 2026-01-01, as `yyyyMMddHHmmss`.
fn timestamp(seq: u64) -> String {
    let s = seq % 86_400;
    format!("20260101{:02}{:02}{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// The decimal result value of record `seq`, such as `5.4`.
pub(crate) fn value_of(seq: u64) -> String {
    format!("{}.{}", 3 + seq % 5, seq % 10)
}

fn padding(seq: u64, len: usize) -> String {
    const WORDS: [&str; 4] = ["alpha", "bravo", "charlie", "delta"];
    let mut text = String::with_capacity(len + 8);
    let mut n = usize::try_from(seq % 4).unwrap_or(0);
    while text.len() < len {
        text.push_str(WORDS[n % 4]);
        text.push(' ');
        n += 1;
    }
    text.truncate(len);
    text
}

/// An HL7 `ORU^R01` of about `size` bytes; the control id is in MSH-10 and the sample id
/// (OBR-3) is the same text. Segments end with CR.
pub(crate) fn hl7_message(plan: &Plan, seq: u64, size: usize) -> String {
    let id = plan.control_id(seq);
    let ts = timestamp(seq);
    let head = [
        format!("MSH|^~\\&|SOAK|LAB|LIS|HOSP|{ts}||ORU^R01|{id}|P|2.5.1"),
        "PID|1||MRN-00000^^^TEST||TEST^PATIENT||19700101|U".to_owned(),
        format!("OBR|1||{id}|GLU^Glucose|||{ts}"),
        format!(
            "OBX|1|NM|GLU^Glucose||{}|mmol/L|3.9-5.6|N|||F",
            value_of(seq)
        ),
    ]
    .join("\r");
    let used = head.len() + "\rOBX|2|TX|NOTE^Note||\r".len();
    let pad = padding(seq, size.saturating_sub(used).max(1));
    format!("{head}\rOBX|2|TX|NOTE^Note||{}\r", pad.trim_end())
}

/// An acknowledgement of `message` with code `code`.
pub(crate) fn hl7_ack(message: &str, code: &str) -> String {
    let id = field(message, "MSH", 10).unwrap_or_default();
    format!(
        "MSH|^~\\&|LIS|HOSP|SOAK|LAB|{}||ACK^R01|A{id}|P|2.5.1\rMSA|{code}|{id}\r",
        timestamp(0)
    )
}

/// Field `n` (HL7 numbering) of the first `segment`; `MSH-1` is the separator.
pub(crate) fn field<'a>(message: &'a str, segment: &str, n: usize) -> Option<&'a str> {
    let line = message
        .split(['\r', '\n'])
        .find(|l| l.split('|').next() == Some(segment))?;
    let index = if segment == "MSH" {
        n.checked_sub(1)?
    } else {
        n
    };
    line.split('|').nth(index)
}

/// The code and the acknowledged control id of an acknowledgement.
pub(crate) fn ack_of(message: &str) -> Option<(&str, &str)> {
    Some((field(message, "MSA", 1)?, field(message, "MSA", 2)?))
}

/// The records of an ASTM result message for `seq`.
pub(crate) fn astm_records(plan: &Plan, seq: u64) -> Vec<String> {
    let id = plan.control_id(seq);
    let ts = timestamp(seq);
    vec![
        format!("H|\\^&|||SOAK^1.0|||||||P|1|{ts}"),
        format!("P|1|MRN-{:05}|||TEST^PATIENT||19700101|F", seq % 1000),
        format!("O|1|{id}||^^^GLU|R"),
        format!("R|1|^^^GLU|{}|mmol/L||N||F||||{ts}", value_of(seq)),
        "L|1|N".to_owned(),
    ]
}

/// The patient id of the ASTM message for `seq`.
pub(crate) fn astm_patient(seq: u64) -> String {
    format!("MRN-{:05}", seq % 1000)
}

fn astm_checksum(bytes: &[u8]) -> String {
    format!("{:02X}", bytes.iter().fold(0_u8, |s, b| s.wrapping_add(*b)))
}

/// `STX FN text ETX C1 C2 CR LF` for a record that fits one frame.
pub(crate) fn astm_frame(number: usize, record: &str) -> Vec<u8> {
    let mut body = vec![b'0' + u8::try_from(number % 8).unwrap_or(0)];
    body.extend_from_slice(record.as_bytes());
    body.push(CR);
    body.push(ETX);
    let mut frame = vec![STX];
    frame.extend_from_slice(&body);
    frame.extend_from_slice(astm_checksum(&body).as_bytes());
    frame.extend_from_slice(&[CR, 0x0A]);
    frame
}

/// A delimited file with one row per sequence number in `seqs`.
pub(crate) fn csv_file(plan: &Plan, seqs: &[u64]) -> String {
    let mut text = String::from("control_id,sample_id,value\n");
    for seq in seqs {
        let id = plan.control_id(*seq);
        text.push_str(&format!("{id},{id},{}\n", value_of(*seq)));
    }
    text
}

/// `(control id, value)` of every data row of a delimited file.
pub(crate) fn parse_csv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let mut cells = line.trim_end_matches('\r').split(',');
            let id = cells.next()?.to_owned();
            let value = cells.nth(1)?.to_owned();
            Some((id, value))
        })
        .collect()
}

fn fnv1a(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(*b)).wrapping_mul(0x0100_0193)
    })
}

/// A newline terminated record of exactly `size` bytes whose length and checksum fields
/// let the sink detect corruption: `R|<token>|<seq>|<length>|<crc>|<padding>`.
pub(crate) fn record_line(plan: &Plan, seq: u64, size: usize) -> Vec<u8> {
    let head_len = format!("R|{}|{seq:09}|{size:05}|00000000|", plan.token()).len();
    let pad = padding(seq, size.saturating_sub(head_len + 1));
    let crc = fnv1a(pad.as_bytes());
    let mut line = format!("R|{}|{seq:09}|{size:05}|{crc:08x}|{pad}", plan.token());
    line.truncate(size.saturating_sub(1));
    line.push('\n');
    line.into_bytes()
}

/// The sequence number of a record line without its newline, `Err` when it is damaged.
pub(crate) fn parse_record_line(plan: &Plan, line: &[u8]) -> Result<u64, String> {
    let text = std::str::from_utf8(line).map_err(|_| "not UTF-8".to_owned())?;
    let mut parts = text.splitn(6, '|');
    let (tag, token, seq, size, crc, pad) = (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    );
    let (Some("R"), Some(token), Some(seq), Some(size), Some(crc), Some(pad)) =
        (tag, token, seq, size, crc, pad)
    else {
        return Err("not a record".to_owned());
    };
    let seq: u64 = seq.parse().map_err(|_| "bad sequence".to_owned())?;
    let size: usize = size.parse().map_err(|_| "bad length".to_owned())?;
    if token != plan.token() || line.len() + 1 != size {
        return Err(format!(
            "wrong token or length ({} of {size})",
            line.len() + 1
        ));
    }
    if format!("{:08x}", fnv1a(pad.as_bytes())) != crc {
        return Err("checksum mismatch".to_owned());
    }
    Ok(seq)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::soak::topology::Kind;

    fn plan(kind: Kind) -> Plan {
        Plan {
            id: "c".to_owned(),
            index: 3,
            kind,
            in_port: 1,
            out_port: 2,
            in_dir: PathBuf::new(),
            out_dir: PathBuf::new(),
            small_buffer: false,
        }
    }

    #[test]
    fn frames_are_cut_from_a_stream() {
        let mut buf = FrameBuf::default();
        let mut bytes = mllp("MSH|a");
        bytes.extend(mllp("MSH|b"));
        buf.push(&bytes[..4]);
        assert_eq!(buf.next_frame(), None);
        buf.push(&bytes[4..]);
        assert_eq!(buf.next_frame().as_deref(), Some("MSH|a"));
        assert_eq!(buf.next_frame().as_deref(), Some("MSH|b"));
    }

    #[test]
    fn hl7_messages_carry_their_id_and_size() {
        let p = plan(Kind::Hl7);
        let m = hl7_message(&p, 7, 600);
        assert_eq!(field(&m, "MSH", 10), Some("H04-000000007"));
        assert_eq!(field(&m, "OBR", 3), Some("H04-000000007"));
        assert_eq!(field(&m, "OBX", 5), Some(value_of(7).as_str()));
        assert!((560..=640).contains(&m.len()), "{}", m.len());
        assert_eq!(m, hl7_message(&p, 7, 600));
    }

    #[test]
    fn an_ack_names_the_acknowledged_message() {
        let p = plan(Kind::Hl7);
        let ack = hl7_ack(&hl7_message(&p, 1, 300), "AA");
        assert_eq!(ack_of(&ack), Some(("AA", "H04-000000001")));
    }

    #[test]
    fn astm_frames_have_a_valid_checksum() {
        let frame = astm_frame(1, "L|1|N");
        let body = &frame[1..frame.len() - 4];
        let sum = astm_checksum(body);
        assert_eq!(&frame[frame.len() - 4..frame.len() - 2], sum.as_bytes());
        assert_eq!(frame[0], STX);
    }

    #[test]
    fn csv_round_trips() {
        let p = plan(Kind::Folder);
        let rows = parse_csv(&csv_file(&p, &[4, 5]));
        assert_eq!(rows[0], (p.control_id(4), value_of(4)));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn records_detect_damage() {
        let p = plan(Kind::Bridge);
        let line = record_line(&p, 12, 256);
        assert_eq!(line.len(), 256);
        assert_eq!(parse_record_line(&p, &line[..255]), Ok(12));
        let mut damaged = line[..255].to_vec();
        damaged[100] ^= 1;
        assert!(parse_record_line(&p, &damaged).is_err());
        assert!(parse_record_line(&p, &line[..200]).is_err());
    }
}
