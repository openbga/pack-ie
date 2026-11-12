use crate::AuError;

/// AU transport class (manual pp. 4-5, 18-22).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransmissionClass {
    /// No acknowledgements: the host never responds to frames.
    A,
    /// The host acknowledges every frame with ACK or NAK.
    B,
}

/// Explicit settings matched to the instrument's online parameter sheet (pp. 5-7, 36).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FramingConfig {
    /// One or two control bytes; ACK/NAK cannot be delimiters.
    pub start: Vec<u8>,
    /// One or two control bytes; ETB substitution applies only to a sole ETX.
    pub end: Vec<u8>,
    /// Whether every frame includes a one-byte XOR BCC.
    pub bcc: bool,
    /// Use ETB for intermediate blocks when the configured end is ETX.
    pub use_etb: bool,
    /// Total wire bytes, including controls and BCC: 256, 512 or 1024.
    pub max_wire_len: u16,
    /// Transmission class; the codec itself never sends a response.
    pub class: TransmissionClass,
}

/// Fixed-width settings; demographics occur only in normal patient messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormatConfig {
    /// Incoming R/D messages have a two-digit unit number when enabled.
    pub unit_no: bool,
    /// Rack width 0, 4 or 5; cup is absent at 0 and two bytes otherwise.
    pub rack_digits: u8,
    /// Exact sample-ID width 1–20; zero-width ambiguity is unsupported.
    pub sample_id_digits: u8,
    /// Whether a normal patient header includes one sex byte.
    pub sex: bool,
    /// Whether a normal patient header includes three age bytes.
    pub age: bool,
    /// Whether a normal patient header includes two month bytes.
    pub month: bool,
    /// Exact widths 0–20 for each of six opaque demographic fields.
    pub patient_info_digits: [u8; 6],
    /// Raw value width 6 or 9, followed by exactly two flag bytes.
    pub result_digits: u8,
    /// Test code width 2 or 3 digits, in result items and in requisition test lists
    /// (online parameter sheet setting for 3-digit test codes); a code of all zeros is invalid.
    pub test_code_digits: u8,
}

/// No defaults: the laboratory must supply its actual instrument settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuConfig {
    /// Frame delimiters, checksum and transmission class.
    pub framing: FramingConfig,
    /// Fixed-width message layout.
    pub format: FormatConfig,
}

impl FormatConfig {
    /// Cup width: absent when the rack is absent, two bytes otherwise.
    pub(crate) fn cup_width(&self) -> usize {
        if self.rack_digits == 0 { 0 } else { 2 }
    }

    /// Width of a result item: test code, raw value and two flag bytes.
    pub(crate) fn result_item_width(&self) -> usize {
        usize::from(self.test_code_digits) + usize::from(self.result_digits) + 2
    }

    /// Widths of the sex, age and month demographic fields.
    pub(crate) fn demographic_widths(&self) -> [usize; 3] {
        [
            usize::from(self.sex),
            if self.age { 3 } else { 0 },
            if self.month { 2 } else { 0 },
        ]
    }
}

impl AuConfig {
    /// Rejects unsupported profiles before processing any external bytes.
    pub fn validate(&self) -> Result<(), AuError> {
        let framing = &self.framing;
        let format = &self.format;
        let control = |bytes: &[u8]| {
            (1..=2).contains(&bytes.len())
                && bytes
                    .iter()
                    .all(|b| (1..=31).contains(b) && !matches!(b, 6 | 21))
        };
        if !control(&framing.start)
            || !control(&framing.end)
            || !matches!(framing.max_wire_len, 256 | 512 | 1024)
            || !matches!(format.rack_digits, 0 | 4 | 5)
            || !(1..=20).contains(&format.sample_id_digits)
            || !matches!(format.result_digits, 6 | 9)
            || !matches!(format.test_code_digits, 2 | 3)
            || format.patient_info_digits.iter().any(|n| *n > 20)
        {
            return Err(AuError::Configuration);
        }
        Ok(())
    }

    pub(crate) fn overhead(&self) -> usize {
        self.framing.start.len() + self.framing.end.len() + usize::from(self.framing.bcc)
    }
    pub(crate) fn etb_enabled(&self) -> bool {
        self.framing.use_etb && self.framing.end == [3]
    }
}
