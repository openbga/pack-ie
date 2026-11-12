//! Borrowed control-segment views and generic acknowledgement construction.
//!
//! Policy follows HL7 v2.5.1 chapter 2, sections 2.9 and 2.15:
//! <https://hl7.eu/HL7v2x/v251/std251/ch02.html>. With both MSH-15/16
//! absent or empty, original mode acknowledges at the application stage only.
//! Otherwise enhanced mode applies; an omitted half requests no ACK at that stage.
//! Generated ACKs leave MSH-15/16 blank to avoid requesting another ACK.
//! The caller supplies identifiers/time and must durably commit before reporting
//! commit acceptance. This module does not perform storage, timing or I/O.

use crate::{Error, Message};

/// A borrowed MSH view. Aggregate identity fields retain structural delimiters.
#[derive(Debug, Clone, Copy)]
pub struct Msh<'a>(&'a Message);

impl<'a> Msh<'a> {
    /// Borrow the MSH of `message`.
    #[must_use]
    pub fn new(message: &'a Message) -> Self {
        Self(message)
    }
    /// MSH-3 sending application.
    #[must_use]
    pub fn sending_application(self) -> Option<&'a str> {
        self.0.get("MSH-3")
    }
    /// MSH-4 sending facility.
    #[must_use]
    pub fn sending_facility(self) -> Option<&'a str> {
        self.0.get("MSH-4")
    }
    /// MSH-5 receiving application.
    #[must_use]
    pub fn receiving_application(self) -> Option<&'a str> {
        self.0.get("MSH-5")
    }
    /// MSH-6 receiving facility.
    #[must_use]
    pub fn receiving_facility(self) -> Option<&'a str> {
        self.0.get("MSH-6")
    }
    /// MSH-7 date/time of message.
    #[must_use]
    pub fn timestamp(self) -> Option<&'a str> {
        self.0.get("MSH-7")
    }
    /// MSH-9.1 message code, for example `ORU`.
    #[must_use]
    pub fn message_type(self) -> Option<&'a str> {
        self.0.get("MSH-9.1")
    }
    /// MSH-9.2 trigger event, for example `R01`.
    #[must_use]
    pub fn trigger_event(self) -> Option<&'a str> {
        self.0.get("MSH-9.2")
    }
    /// MSH-10 message control id.
    #[must_use]
    pub fn control_id(self) -> Option<&'a str> {
        self.0.get("MSH-10")
    }
    /// MSH-11 processing id.
    #[must_use]
    pub fn processing_id(self) -> Option<&'a str> {
        self.0.get("MSH-11")
    }
    /// MSH-12.1 version id.
    #[must_use]
    pub fn version(self) -> Option<&'a str> {
        self.0.get("MSH-12.1")
    }
    /// MSH-15 accept acknowledgement type.
    #[must_use]
    pub fn accept_ack_type(self) -> Option<&'a str> {
        self.0.get("MSH-15")
    }
    /// MSH-16 application acknowledgement type.
    #[must_use]
    pub fn application_ack_type(self) -> Option<&'a str> {
        self.0.get("MSH-16")
    }
}

/// A borrowed first MSA segment, when present.
#[derive(Debug, Clone, Copy)]
pub struct Msa<'a>(&'a Message);

impl<'a> Msa<'a> {
    /// Borrow the first MSA of `message`; `None` when there is none.
    #[must_use]
    pub fn new(message: &'a Message) -> Option<Self> {
        message.segment("MSA", 1).map(|_| Self(message))
    }
    /// MSA-1 acknowledgement code.
    #[must_use]
    pub fn code(self) -> Option<&'a str> {
        self.0.get("MSA-1")
    }
    /// MSA-2 control id of the acknowledged message.
    #[must_use]
    pub fn control_id(self) -> Option<&'a str> {
        self.0.get("MSA-2")
    }
    /// MSA-3 text message.
    #[must_use]
    pub fn text(self) -> Option<&'a str> {
        self.0.get("MSA-3")
    }
}

/// A borrowed first ERR segment; legacy and modern locations are distinct.
#[derive(Debug, Clone, Copy)]
pub struct ErrSegment<'a>(&'a Message);

impl<'a> ErrSegment<'a> {
    /// Borrow the first ERR of `message`; `None` when there is none.
    #[must_use]
    pub fn new(message: &'a Message) -> Option<Self> {
        message.segment("ERR", 1).map(|_| Self(message))
    }
    /// ERR-1 error code and location (versions before 2.5).
    #[must_use]
    pub fn legacy_code_and_location(self) -> Option<&'a str> {
        self.0.get("ERR-1")
    }
    /// ERR-2 error location.
    #[must_use]
    pub fn location(self) -> Option<&'a str> {
        self.0.get("ERR-2")
    }
    /// ERR-3.1 error code identifier.
    #[must_use]
    pub fn code(self) -> Option<&'a str> {
        self.0.get("ERR-3.1")
    }
    /// ERR-4 severity.
    #[must_use]
    pub fn severity(self) -> Option<&'a str> {
        self.0.get("ERR-4")
    }
    /// ERR-7 diagnostic information.
    #[must_use]
    pub fn diagnostic(self) -> Option<&'a str> {
        self.0.get("ERR-7")
    }
    /// ERR-8 user message.
    #[must_use]
    pub fn user_message(self) -> Option<&'a str> {
        self.0.get("ERR-8")
    }
}

/// Requested acknowledgement condition (HL7 table 0155).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckPolicy {
    /// `AL`: always acknowledge.
    Always,
    /// `NE`, or absent: never acknowledge.
    Never,
    /// `ER`: acknowledge errors and rejections only.
    Error,
    /// `SU`: acknowledge successful outcomes only.
    Success,
}

impl AckPolicy {
    /// An absent/empty policy in enhanced mode means no ACK at that stage.
    /// Parse an MSH-15/16 value.
    ///
    /// # Errors
    ///
    /// [`AckError::InvalidPolicy`] for a value outside table 0155.
    pub fn parse(value: Option<&str>) -> Result<Self, AckError> {
        match value {
            Some("AL") => Ok(Self::Always),
            None | Some("" | "NE") => Ok(Self::Never),
            Some("ER") => Ok(Self::Error),
            Some("SU") => Ok(Self::Success),
            _ => Err(AckError::InvalidPolicy),
        }
    }

    /// Whether this policy asks for an acknowledgement of `outcome`.
    #[must_use]
    pub fn requests(self, outcome: AckOutcome) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Error => outcome != AckOutcome::Accepted,
            Self::Success => outcome == AckOutcome::Accepted,
        }
    }
}

/// Acknowledgement mode of a received message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckMode {
    /// MSH-15/16 are absent or empty: only the application stage is acknowledged.
    Original,
    /// MSH-15/16 request acknowledgements per stage.
    Enhanced {
        /// Policy for commit acknowledgements (MSH-15).
        accept: AckPolicy,
        /// Policy for application acknowledgements (MSH-16).
        application: AckPolicy,
    },
}

impl AckMode {
    /// Select the mode from MSH-15/16 of a received message.
    ///
    /// # Errors
    ///
    /// [`AckError::InvalidPolicy`] when MSH-15/16 holds an unknown or non-UTF-8 value.
    pub fn from_message(message: &Message) -> Result<Self, AckError> {
        let msh = Msh::new(message);
        for field in [15, 16] {
            if message
                .msh_field(field)
                .is_some_and(|field| field.value().is_none())
            {
                return Err(AckError::InvalidPolicy);
            }
        }
        let accept = msh.accept_ack_type().filter(|value| !value.is_empty());
        let application = msh.application_ack_type().filter(|value| !value.is_empty());
        if accept.is_none() && application.is_none() {
            return Ok(Self::Original);
        }
        Ok(Self::Enhanced {
            accept: AckPolicy::parse(accept)?,
            application: AckPolicy::parse(application)?,
        })
    }

    /// Whether the mode asks for an acknowledgement at `stage` for `outcome`.
    #[must_use]
    pub fn requests(self, stage: AckStage, outcome: AckOutcome) -> bool {
        match (self, stage) {
            (Self::Original, AckStage::Commit) => false,
            (Self::Original, AckStage::Application) => true,
            (Self::Enhanced { accept, .. }, AckStage::Commit) => accept.requests(outcome),
            (Self::Enhanced { application, .. }, AckStage::Application) => {
                application.requests(outcome)
            }
        }
    }
}

/// Processing stage an acknowledgement reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckStage {
    /// Commit acceptance: `CA`, `CE` or `CR`.
    Commit,
    /// Application acceptance: `AA`, `AE` or `AR`.
    Application,
}

/// Result an acknowledgement reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckOutcome {
    /// The message was accepted.
    Accepted,
    /// The message was received but processing failed.
    Error,
    /// The message was refused.
    Rejected,
}

impl AckOutcome {
    /// The MSA-1 acknowledgement code for this outcome at `stage`.
    #[must_use]
    pub fn code(self, stage: AckStage) -> &'static str {
        match (stage, self) {
            (AckStage::Commit, Self::Accepted) => "CA",
            (AckStage::Commit, Self::Error) => "CE",
            (AckStage::Commit, Self::Rejected) => "CR",
            (AckStage::Application, Self::Accepted) => "AA",
            (AckStage::Application, Self::Error) => "AE",
            (AckStage::Application, Self::Rejected) => "AR",
        }
    }
}

/// Meaning of a received MSA-1 acknowledgement code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckClassification {
    /// `AA`.
    Accepted,
    /// `AE`.
    Error,
    /// `AR`.
    Rejected,
    /// `CA`.
    CommitAccepted,
    /// `CE`.
    CommitError,
    /// `CR`.
    CommitRejected,
    /// Any other code; never treated as success.
    Unknown,
}

impl AckClassification {
    /// Classify an MSA-1 code.
    #[must_use]
    pub fn from_code(code: &str) -> Self {
        match code {
            "AA" => Self::Accepted,
            "AE" => Self::Error,
            "AR" => Self::Rejected,
            "CA" => Self::CommitAccepted,
            "CE" => Self::CommitError,
            "CR" => Self::CommitRejected,
            _ => Self::Unknown,
        }
    }
}

/// Parsed general ACK. Unknown MSA codes stay explicit rather than succeeding.
#[derive(Debug, Clone, Copy)]
pub struct Ack<'a> {
    /// Meaning of MSA-1.
    pub classification: AckClassification,
    /// MSA-2, the control id of the acknowledged message.
    pub control_id: &'a str,
    /// MSA-3, when present.
    pub text: Option<&'a str>,
}

impl<'a> Ack<'a> {
    /// View a general acknowledgement.
    ///
    /// # Errors
    ///
    /// [`AckError::NotAcknowledgement`] unless MSH-9.1 is `ACK`, and [`AckError::MissingField`] when MSA, MSA-1 or MSA-2 is absent, empty or not UTF-8.
    pub fn from_message(message: &'a Message) -> Result<Self, AckError> {
        Self::from_reply(message, &["ACK"])
    }

    /// View an acknowledgement whose MSH-9.1 is one of `message_types`. Order messages
    /// are answered by order responses (`ORL`, `ORR`) that carry the same MSA segment.
    ///
    /// # Errors
    ///
    /// [`AckError::NotAcknowledgement`] unless MSH-9.1 is listed, and
    /// [`AckError::MissingField`] when MSA, MSA-1 or MSA-2 is absent, empty or not UTF-8.
    pub fn from_reply(message: &'a Message, message_types: &[&str]) -> Result<Self, AckError> {
        if !Msh::new(message)
            .message_type()
            .is_some_and(|kind| message_types.contains(&kind))
        {
            return Err(AckError::NotAcknowledgement);
        }
        let msa = Msa::new(message).ok_or(AckError::MissingField("MSA"))?;
        let code = required(msa.code(), "MSA-1")?;
        Ok(Self {
            classification: AckClassification::from_code(code),
            control_id: required(msa.control_id(), "MSA-2")?,
            text: msa.text(),
        })
    }

    /// Compare MSA-2 with the control id of the message being acknowledged.
    /// Compare MSA-2 with the control id of the message being acknowledged.
    ///
    /// # Errors
    ///
    /// [`AckError::CorrelationMismatch`] when they differ.
    pub fn validate_correlation(self, expected_control_id: &str) -> Result<(), AckError> {
        if self.control_id == expected_control_id {
            Ok(())
        } else {
            Err(AckError::CorrelationMismatch)
        }
    }
}

/// Acknowledgement failure. Only structural metadata appears in errors, never
/// field or message contents. Wraps [`Error`] rather than being merged with it:
/// `Error` describes message structure and is shared by parsing and editing,
/// while this enum adds acknowledgement policy outcomes.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AckError {
    /// MSH-15/16 holds a value outside HL7 table 0155.
    #[error("invalid acknowledgement policy")]
    InvalidPolicy,
    /// A required field is absent, empty or not UTF-8.
    #[error("missing or undecodable acknowledgement field {0}")]
    MissingField(&'static str),
    /// The message is not an `ACK`.
    #[error("message is not a general acknowledgement")]
    NotAcknowledgement,
    /// MSA-2 differs from the sent control id.
    #[error("acknowledgement control id does not match the sent message")]
    CorrelationMismatch,
    /// The caller supplied an empty control id or timestamp.
    #[error("ACK control id and timestamp must be nonempty")]
    MissingReplyIdentity,
    /// MSH-12 is not a version whose ERR layout is known.
    #[error("unsupported HL7 version for ERR encoding")]
    UnsupportedErrorVersion,
    /// Building the reply failed in the message model.
    #[error(transparent)]
    Encoding(#[from] Error),
}

/// ERR-4 severity (HL7 table 0516).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    /// `I`.
    Information,
    /// `W`.
    Warning,
    /// `E`.
    Error,
    /// `F`.
    Fatal,
}

impl ErrorSeverity {
    /// The ERR-4 code.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::Information => "I",
            Self::Warning => "W",
            Self::Error => "E",
            Self::Fatal => "F",
        }
    }
}

/// Generic HL7 error location. Positions are one-based; omission leaves a component empty.
#[derive(Debug, Clone, Copy, Default)]
pub struct ErrorLocation<'a> {
    /// Segment name.
    pub segment: &'a str,
    /// Segment occurrence.
    pub segment_sequence: Option<usize>,
    /// Field number.
    pub field: Option<usize>,
    /// Field repetition.
    pub repetition: Option<usize>,
    /// Component number.
    pub component: Option<usize>,
    /// Subcomponent number.
    pub subcomponent: Option<usize>,
}

/// Optional ERR details. Code is from HL7 table 0357; supplied text is literal.
#[derive(Debug, Clone, Copy)]
pub struct ErrorDetails<'a> {
    /// Error code (table 0357) or its legacy equivalent.
    pub code: &'a str,
    /// Code text.
    pub description: Option<&'a str>,
    /// Coding system name.
    pub coding_system: Option<&'a str>,
    /// Severity; carried by version 2.5 and later only.
    pub severity: ErrorSeverity,
    /// Where the error occurred.
    pub location: Option<ErrorLocation<'a>>,
    /// Free-text diagnostic; version 2.5 and later only.
    pub diagnostic: Option<&'a str>,
    /// Text for the end user; version 2.5 and later only.
    pub user_message: Option<&'a str>,
}

/// Caller-supplied content of a generated acknowledgement.
#[derive(Debug, Clone, Copy)]
pub struct AckOptions<'a> {
    /// MSH-10 of the reply; nonempty.
    pub control_id: &'a str,
    /// MSH-7 of the reply; nonempty.
    pub timestamp: &'a str,
    /// MSA-3 text.
    pub text: Option<&'a str>,
    /// ERR details.
    pub error: Option<ErrorDetails<'a>>,
}

/// Build an ACK when the source's policy requests it; otherwise return `None`.
/// Original mode returns only application ACKs. Commit acceptance is a caller
/// assertion that durable storage has already succeeded, not a storage action.
///
/// # Errors
///
/// [`AckError::InvalidPolicy`] for an unusable MSH-15/16. When an ACK is requested:
/// [`AckError::MissingReplyIdentity`] for an empty id or timestamp,
/// [`AckError::MissingField`] when the source lacks MSH-10, MSH-11 or MSH-12 (or
/// an identity field is not UTF-8), and [`AckError::UnsupportedErrorVersion`] when
/// error details are supplied for an unknown version.
pub fn build_ack(
    source: &Message,
    stage: AckStage,
    outcome: AckOutcome,
    options: &AckOptions<'_>,
) -> Result<Option<Message>, AckError> {
    if !AckMode::from_message(source)?.requests(stage, outcome) {
        return Ok(None);
    }
    if options.control_id.is_empty() || options.timestamp.is_empty() {
        return Err(AckError::MissingReplyIdentity);
    }
    let msh = Msh::new(source);
    let control_id = required(msh.control_id(), "MSH-10")?;
    let version = required(msh.version(), "MSH-12")?;
    required(msh.processing_id(), "MSH-11")?;
    let mut reply = Message::new(source.encoding());
    for (from, to) in [(3, 5), (4, 6), (5, 3), (6, 4), (11, 11), (12, 12)] {
        copy_header_field(source, &mut reply, from, to)?;
    }
    reply.set("MSH-7", options.timestamp)?;
    reply.set("MSH-9.1", "ACK")?;
    if let Some(trigger) = msh.trigger_event().filter(|value| !value.is_empty()) {
        reply.set("MSH-9.2", trigger)?;
    }
    // Message structure identifiers are defined from v2.3.1 onward.
    if !matches!(version, "2.1" | "2.2" | "2.3") {
        reply.set("MSH-9.3", "ACK")?;
    }
    reply.set("MSH-10", options.control_id)?;
    reply.set("MSA-1", outcome.code(stage))?;
    reply.set("MSA-2", control_id)?;
    if let Some(text) = options.text {
        reply.set("MSA-3", text)?;
    }
    if let Some(error) = options.error {
        write_error(&mut reply, version, error)?;
    }
    Ok(Some(reply))
}

/// The order response that answers an order message of `message_type` with `trigger`:
/// `OML^O33` gets `ORL^O34`, `OML^O35` `ORL^O36`, `OML^O21` `ORL^O22` and `ORM^O01`
/// `ORR^O02`. Returns the response's message code and trigger event.
#[must_use]
pub fn order_response_type(
    message_type: &str,
    trigger: &str,
) -> Option<(&'static str, &'static str)> {
    match (message_type, trigger) {
        ("OML", "O33") => Some(("ORL", "O34")),
        ("OML", "O35") => Some(("ORL", "O36")),
        ("OML", "O21") => Some(("ORL", "O22")),
        ("ORM", "O01") => Some(("ORR", "O02")),
        _ => None,
    }
}

/// Turns a generated `ack` of `source` into the order response of its message type
/// (see [`order_response_type`]): MSH-9 becomes `ORL^O34^ORL_O34` and so on (without the
/// structure component before v2.3.1, as in [`build_ack`]). MSA and ERR stay as built: the
/// result is the base HL7 response of MSH and MSA, which these structures allow, with no
/// response group (ORC), so it states no per-order acceptance and is not an IHE LAW LAB-28
/// response.
/// Returns whether `source` is an order message; otherwise `ack` is unchanged.
///
/// # Errors
///
/// [`AckError::Encoding`] when the reply cannot be edited.
pub fn into_order_response(ack: &mut Message, source: &Message) -> Result<bool, AckError> {
    let msh = Msh::new(source);
    let (Some(kind), Some(trigger)) = (msh.message_type(), msh.trigger_event()) else {
        return Ok(false);
    };
    let Some((code, event)) = order_response_type(kind, trigger) else {
        return Ok(false);
    };
    ack.set("MSH-9.1", code)?;
    ack.set("MSH-9.2", event)?;
    if ack.get("MSH-9.3").is_some() {
        ack.set("MSH-9.3", &format!("{code}_{event}"))?;
    }
    Ok(true)
}

fn required<'a>(value: Option<&'a str>, field: &'static str) -> Result<&'a str, AckError> {
    value
        .filter(|value| !value.is_empty())
        .ok_or(AckError::MissingField(field))
}

fn copy_header_field(
    source: &Message,
    target: &mut Message,
    from: usize,
    to: usize,
) -> Result<(), AckError> {
    let Some(field) = source.msh_field(from) else {
        return Ok(());
    };
    for (r, repetition) in field.repetitions().iter().enumerate() {
        for (c, component) in repetition.components().iter().enumerate() {
            for (s, subcomponent) in component.subcomponents().iter().enumerate() {
                let value = subcomponent
                    .value()
                    .ok_or(AckError::MissingField("MSH identity"))?;
                target.set(&format!("MSH-{to}[{r}].{}.{}", c + 1, s + 1), value)?;
            }
        }
    }
    Ok(())
}

fn write_error(
    reply: &mut Message,
    version: &str,
    error: ErrorDetails<'_>,
) -> Result<(), AckError> {
    match version {
        "2.1" | "2.2" | "2.3" | "2.3.1" | "2.4" => write_legacy_error(reply, error)?,
        _ if modern_error_version(version) => write_modern_error(reply, error)?,
        _ => return Err(AckError::UnsupportedErrorVersion),
    }
    Ok(())
}

fn modern_error_version(version: &str) -> bool {
    let mut parts = version.split('.');
    parts.next() == Some("2")
        && parts
            .next()
            .and_then(|part| part.parse::<u16>().ok())
            .is_some_and(|minor| minor >= 5)
}

fn write_legacy_error(reply: &mut Message, error: ErrorDetails<'_>) -> Result<(), Error> {
    if let Some(location) = error.location {
        reply.set("ERR-1.1", location.segment)?;
        set_number(reply, "ERR-1.2", location.segment_sequence)?;
        set_number(reply, "ERR-1.3", location.field)?;
    }
    set_coded_element(reply, "ERR-1.4", error)
}

fn write_modern_error(reply: &mut Message, error: ErrorDetails<'_>) -> Result<(), Error> {
    if let Some(location) = error.location {
        reply.set("ERR-2.1", location.segment)?;
        for (component, value) in [
            (2, location.segment_sequence),
            (3, location.field),
            (4, location.repetition),
            (5, location.component),
            (6, location.subcomponent),
        ] {
            set_number(reply, &format!("ERR-2.{component}"), value)?;
        }
    }
    set_coded_element(reply, "ERR-3", error)?;
    reply.set("ERR-4", error.severity.code())?;
    if let Some(text) = error.diagnostic {
        reply.set("ERR-7", text)?;
    }
    if let Some(text) = error.user_message {
        reply.set("ERR-8", text)?;
    }
    Ok(())
}

/// Write the error code, text and coding system as components 1 to 3 of `path`.
fn set_coded_element(
    reply: &mut Message,
    path: &str,
    error: ErrorDetails<'_>,
) -> Result<(), Error> {
    reply.set(&format!("{path}.1"), error.code)?;
    if let Some(text) = error.description {
        reply.set(&format!("{path}.2"), text)?;
    }
    if let Some(system) = error.coding_system {
        reply.set(&format!("{path}.3"), system)?;
    }
    Ok(())
}

fn set_number(message: &mut Message, path: &str, value: Option<usize>) -> Result<(), Error> {
    if let Some(value) = value {
        message.set(path, &value.to_string())?;
    }
    Ok(())
}
