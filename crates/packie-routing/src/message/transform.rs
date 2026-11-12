//! Filters and mappings of one message channel definition, compiled with `packie-transform`
//! (`docs/spec/configuration.md` §8, `docs/spec/message-channels.md` §6), and the codecs
//! that turn original bytes into a model and a model into a destination's bytes.
//!
//! A definition is compiled when it is applied (problems fail the apply) and again by
//! the running instance whenever the definition's version changes, so in-place updates
//! take effect between messages.

use std::collections::HashMap;

use packie_au::{
    AssemblyOutcome, AuConfig, FrameEnd, Message as AuMessage, ResultAssembler, SampleResults,
    StreamDecoder, WireEvent,
};
use packie_config::{
    Charset, ColumnType, DelimitedSettings, Destination, Hl7Settings, LineEnding, MessageChannel,
    OnExists, ProtocolSettings, RecordMode,
};
use packie_core::{ConnectorId, MessageId, ProtocolKind, Timestamp};
use packie_transform::{
    AstmModel, AuModel, Filters, Hl7Model, Mapping, Model, Problem, RecordModel, Schema,
};

use crate::message::store::Identifiers;
use crate::message::{astm, au, hl7};

/// How the source's bytes are parsed.
enum SourceFormat {
    Hl7,
    Astm(astm::SamplePosition),
    Au(AuConfig),
    Delimited(packie_files::Template),
}

/// How a destination's output is built and encoded.
enum TargetFormat {
    Hl7(Box<Hl7Settings>),
    Astm,
    Au(AuConfig),
    Delimited(packie_files::Template),
    Raw,
}

struct CompiledDestination {
    filters: Filters,
    mapping: Option<Mapping>,
    target: TargetFormat,
}

/// One definition's compiled filters and mappings, with the version they came from.
pub(crate) struct Compiled {
    pub(crate) version: u64,
    source: SourceFormat,
    filters: Filters,
    destinations: HashMap<ConnectorId, CompiledDestination>,
}

/// A source message parsed into the model filters and mappings read.
#[derive(Debug, Clone)]
pub(crate) enum Parsed {
    Hl7(Hl7Model),
    Astm(AstmModel),
    Au(AuModel),
    Records(RecordModel),
}

impl Parsed {
    fn model(&self) -> &dyn Model {
        match self {
            Self::Hl7(m) => m,
            Self::Astm(m) => m,
            Self::Au(m) => m,
            Self::Records(m) => m,
        }
    }
}

/// A mapping or encoding failure (`transform_error`): names the rule or condition, never
/// a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OutputError(pub(crate) String);

impl Compiled {
    /// Compiles `channel`; every problem carries its JSON path under `at` (for example
    /// `channels[2]`).
    pub(crate) fn compile(
        channel: &MessageChannel,
        version: u64,
        at: &str,
    ) -> Result<Self, Vec<Problem>> {
        let schema = channel.source.protocol.schema();
        let mut problems = Vec::new();
        let filters = Filters::compile(&channel.filters, &schema, &format!("{at}.filters"))
            .map_err(|p| problems.extend(p))
            .ok();
        let mut destinations = HashMap::new();
        for (index, destination) in channel.destinations.iter().enumerate() {
            let at = format!("{at}.destinations[{index}]");
            match compile_destination(destination, &schema, &at) {
                Ok(compiled) => {
                    destinations.insert(destination.id, compiled);
                }
                Err(p) => problems.extend(p),
            }
        }
        match filters {
            Some(filters) if problems.is_empty() => Ok(Self {
                version,
                source: source_format(&channel.source.protocol),
                filters,
                destinations,
            }),
            _ => Err(problems),
        }
    }

    /// Parses original bytes with their identifiers; `Err` is the `invalid` reason code.
    pub(crate) fn parse(&self, original: &[u8]) -> Result<(Parsed, Identifiers), String> {
        match &self.source {
            SourceFormat::Hl7 => {
                let message = hl7::verdict(original).map_err(str::to_owned)?;
                let identifiers = hl7::identifiers(&message);
                Ok((Parsed::Hl7(Hl7Model::from_message(message)), identifiers))
            }
            SourceFormat::Astm(sample) => {
                let identifiers = astm::identifiers(original, *sample).map_err(str::to_owned)?;
                let model = AstmModel::parse(original).map_err(|_| "unparseable".to_owned())?;
                Ok((Parsed::Astm(model), identifiers))
            }
            SourceFormat::Au(config) => {
                let results =
                    au_results(config, original).ok_or_else(|| "unparseable".to_owned())?;
                let identifiers = au::identifiers(&results, "");
                Ok((Parsed::Au(AuModel::from_results(&results)), identifiers))
            }
            SourceFormat::Delimited(template) => {
                let rows = delimited_rows(original, template)
                    .map_err(|e| delimited_reason(&e).to_owned())?;
                let identifiers = delimited_identifiers(rows.first().map(Vec::as_slice));
                Ok((Parsed::Records(RecordModel::from_rows(rows)), identifiers))
            }
        }
    }

    /// The channel's filters.
    pub(crate) fn source_passes(&self, message: &Parsed) -> bool {
        self.filters.passes(message.model())
    }

    /// The destination's filters; a destination this definition does not know does not
    /// pass.
    pub(crate) fn destination_passes(&self, destination: ConnectorId, message: &Parsed) -> bool {
        self.destinations
            .get(&destination)
            .is_some_and(|d| d.filters.passes(message.model()))
    }

    /// The destination's output: without a mapping the original bytes (AU re-framed for
    /// the destination's profile), otherwise the mapping applied to a new target and
    /// encoded for the destination's protocol.
    pub(crate) fn output(
        &self,
        destination: ConnectorId,
        message: &Parsed,
        original: &[u8],
    ) -> Result<Vec<u8>, OutputError> {
        let Some(compiled) = self.destinations.get(&destination) else {
            return Err(OutputError(
                "the destination is not in the active configuration".into(),
            ));
        };
        match (&compiled.mapping, &compiled.target) {
            (None, TargetFormat::Au(target)) => self.reframe_au(original, target),
            (None, _) => Ok(original.to_vec()),
            (Some(mapping), target) => apply(mapping, message, target),
        }
    }

    /// AU text passes through unchanged; only the framing follows the destination.
    fn reframe_au(&self, original: &[u8], target: &AuConfig) -> Result<Vec<u8>, OutputError> {
        let SourceFormat::Au(source) = &self.source else {
            return Err(OutputError(
                "encode: an AU destination sends AU messages; the source is not AU".into(),
            ));
        };
        let frames = au_frames(source, original)
            .ok_or_else(|| OutputError("encode: the AU frames do not decode".into()))?;
        let mut out = Vec::new();
        for (text, end) in frames {
            let wire = packie_au::encode_frame(target, &text, end).map_err(|e| {
                OutputError(format!(
                    "encode: the AU frame does not fit the destination profile ({e})"
                ))
            })?;
            out.extend_from_slice(&wire);
        }
        Ok(out)
    }
}

/// Runs `mapping` into a new target of the destination's protocol and encodes it. A
/// same-protocol target starts as a copy of the source.
fn apply(
    mapping: &Mapping,
    message: &Parsed,
    target: &TargetFormat,
) -> Result<Vec<u8>, OutputError> {
    let source = message.model();
    let fail = |e: packie_transform::TransformError| OutputError(e.to_string());
    match target {
        TargetFormat::Hl7(settings) => {
            let mut target = match message {
                Parsed::Hl7(model) => model.clone(),
                _ => new_hl7(settings),
            };
            mapping.apply(source, &mut target).map_err(fail)?;
            Ok(target.encode())
        }
        TargetFormat::Astm => {
            let mut target = match message {
                Parsed::Astm(model) => model.clone(),
                _ => AstmModel::new(),
            };
            mapping.apply(source, &mut target).map_err(fail)?;
            Ok(target.encode())
        }
        TargetFormat::Delimited(template) => {
            let mut target = RecordModel::new();
            mapping.apply(source, &mut target).map_err(fail)?;
            encode_rows(&target, template)
        }
        TargetFormat::Au(_) | TargetFormat::Raw => Err(OutputError(
            "mapping: the destination protocol cannot be a mapping target".into(),
        )),
    }
}

/// A new HL7 message with MSH-3..7 and MSH-10..12 from the connector settings.
fn new_hl7(settings: &Hl7Settings) -> Hl7Model {
    let mut model = Hl7Model::new();
    let schema = Schema::hl7();
    let control_id = MessageId::generate().to_string();
    let timestamp = hl7::hl7_timestamp(Timestamp::now());
    let processing = match settings.processing_id {
        packie_config::ProcessingId::P => "P",
        packie_config::ProcessingId::D => "D",
        packie_config::ProcessingId::T => "T",
    };
    let fields = [
        ("MSH.3", settings.sending_application.as_str()),
        ("MSH.4", settings.sending_facility.as_str()),
        ("MSH.5", settings.receiving_application.as_str()),
        ("MSH.6", settings.receiving_facility.as_str()),
        ("MSH.7", timestamp.as_str()),
        ("MSH.10", control_id.as_str()),
        ("MSH.11", processing),
        ("MSH.12", settings.version.as_str()),
    ];
    for (path, value) in fields.into_iter().filter(|(_, v)| !v.is_empty()) {
        if let Ok(path) = schema.parse(path) {
            // The header fields are writable; a refused value leaves the field empty.
            model.set(&path, value).ok();
        }
    }
    model
}

/// Rows of a mapped record model, written with the destination template.
fn encode_rows(
    target: &RecordModel,
    template: &packie_files::Template,
) -> Result<Vec<u8>, OutputError> {
    let mut records = Vec::with_capacity(target.rows().len());
    for row in target.rows() {
        let mut record = packie_files::Record::empty(template)
            .map_err(|e| OutputError(format!("encode: {e}")))?;
        for (name, value) in row {
            record
                .set(&format!("col.{name}"), value)
                .map_err(|e| OutputError(format!("encode: {e}")))?;
        }
        records.push(record);
    }
    packie_files::write(&records, template).map_err(|e| OutputError(format!("encode: {e}")))
}

fn compile_destination(
    destination: &Destination,
    source: &Schema,
    at: &str,
) -> Result<CompiledDestination, Vec<Problem>> {
    let mut problems = Vec::new();
    let filters = Filters::compile(&destination.filters, source, &format!("{at}.filters"))
        .map_err(|p| problems.extend(p))
        .ok();
    let mapping = destination.mapping.as_ref().and_then(|mapping| {
        let target = destination.protocol.schema();
        Mapping::compile(mapping, source, &target, &format!("{at}.mapping"))
            .map_err(|p| problems.extend(p))
            .ok()
    });
    match filters {
        Some(filters) if problems.is_empty() => Ok(CompiledDestination {
            filters,
            mapping,
            target: target_format(&destination.protocol),
        }),
        _ => Err(problems),
    }
}

fn source_format(protocol: &ProtocolSettings) -> SourceFormat {
    match protocol {
        ProtocolSettings::Hl7(_) | ProtocolSettings::Raw => SourceFormat::Hl7,
        ProtocolSettings::Astm(settings) => SourceFormat::Astm(astm::sample_position(settings)),
        ProtocolSettings::Au(settings) => SourceFormat::Au(au::config(settings).0),
        ProtocolSettings::Delimited(settings) => SourceFormat::Delimited(stored_template(settings)),
    }
}

fn target_format(protocol: &ProtocolSettings) -> TargetFormat {
    match protocol {
        ProtocolSettings::Hl7(settings) => TargetFormat::Hl7(Box::new(settings.clone())),
        ProtocolSettings::Astm(_) => TargetFormat::Astm,
        ProtocolSettings::Au(settings) => TargetFormat::Au(au::config(settings).0),
        ProtocolSettings::Delimited(settings) => TargetFormat::Delimited(template(settings)),
        ProtocolSettings::Raw => TargetFormat::Raw,
    }
}

/// The codec template of a delimited connector (`configuration.md` §7.4).
pub(crate) fn template(settings: &DelimitedSettings) -> packie_files::Template {
    packie_files::Template {
        delimiter: settings.delimiter.clone(),
        quote: settings.quote.clone(),
        header_row: settings.header_row,
        skip_lines: settings.skip_lines,
        charset: match settings.charset {
            Charset::Utf8 => packie_files::Charset::Utf8,
            Charset::Latin1 => packie_files::Charset::Latin1,
            Charset::Ascii => packie_files::Charset::Ascii,
        },
        line_ending: match settings.line_ending {
            LineEnding::Crlf => packie_files::LineEnding::Crlf,
            LineEnding::Lf => packie_files::LineEnding::Lf,
        },
        write_bom: settings.write_bom,
        columns: settings.columns.iter().map(column).collect(),
        record_mode: match settings.record_mode {
            RecordMode::OnePerLine => packie_files::RecordMode::OnePerLine,
            RecordMode::Grouped => packie_files::RecordMode::Grouped,
        },
        group_by: settings.group_by.clone(),
        max_file_bytes: settings.max_file_bytes,
        file_name: settings.file_name.clone(),
        on_exists: match settings.on_exists {
            OnExists::Suffix => packie_files::OnExists::Suffix,
            OnExists::Overwrite => packie_files::OnExists::Overwrite,
            OnExists::Append => packie_files::OnExists::Append,
            OnExists::Fail => packie_files::OnExists::Fail,
        },
    }
}

/// The template a stored delimited message is written and read back with: its original
/// is its rows written with the source's columns, so no leading lines are skipped and no
/// BOM is written. The source's `max_file_bytes` already bounded the file it came from;
/// written again (line endings, quoting) a group may differ slightly in size, so the
/// stored form takes the codec's largest limit rather than refusing a file that was read.
pub(crate) fn stored_template(settings: &DelimitedSettings) -> packie_files::Template {
    packie_files::Template {
        skip_lines: 0,
        write_bom: false,
        max_file_bytes: packie_files::Template::MAX_FILE_BYTES,
        ..template(settings)
    }
}

fn column(c: &packie_config::Column) -> packie_files::Column {
    packie_files::Column {
        name: c.name.clone(),
        header: c.header.clone(),
        position: c.position,
        column_type: match c.column_type {
            ColumnType::String => packie_files::ColumnType::String,
            ColumnType::Integer => packie_files::ColumnType::Integer,
            ColumnType::Decimal => packie_files::ColumnType::Decimal,
            ColumnType::Date => packie_files::ColumnType::Date,
            ColumnType::Datetime => packie_files::ColumnType::Datetime,
        },
        format: c.format.clone(),
        required: c.required,
    }
}

type Row = Vec<(String, String)>;

/// The rows of delimited bytes as `(column, text)` pairs.
fn delimited_rows(
    bytes: &[u8],
    template: &packie_files::Template,
) -> Result<Vec<Row>, packie_files::Error> {
    let document = packie_files::read(bytes, template)?;
    Ok(document.records.iter().map(row_of).collect())
}

/// One record as `(column, text)` pairs.
pub(crate) fn row_of(record: &packie_files::Record) -> Row {
    record
        .fields()
        .iter()
        .map(|f| (f.name().to_owned(), f.value().as_str().to_owned()))
        .collect()
}

/// The identifiers of a delimited message: the columns named `control_id`, `sample_id`
/// and `patient_id` of its first row.
pub(crate) fn delimited_identifiers(first_row: Option<&[(String, String)]>) -> Identifiers {
    let column = |name: &str| {
        first_row
            .and_then(|row| row.iter().find(|(n, _)| n == name))
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    };
    Identifiers {
        control_id: column("control_id"),
        sample_id: column("sample_id"),
        patient_id: column("patient_id"),
        message_type: Some("delimited".to_owned()),
        source_identity: None,
    }
}

/// The reason code of delimited bytes that cannot be read.
pub(crate) const fn delimited_reason(error: &packie_files::Error) -> &'static str {
    match error {
        packie_files::Error::FileTooLarge { .. } => "file_too_large",
        packie_files::Error::Encoding { .. } | packie_files::Error::Unrepresentable { .. } => {
            "encoding"
        }
        _ => "malformed_file",
    }
}

/// The text and end of each frame in AU wire bytes; `None` when any part fails to decode.
pub(crate) fn au_frames(config: &AuConfig, original: &[u8]) -> Option<Vec<(Vec<u8>, FrameEnd)>> {
    let mut decoder = StreamDecoder::new(config.clone()).ok()?;
    let mut frames = Vec::new();
    for event in decoder.push(original) {
        match event.ok()? {
            WireEvent::Frame(frame) => frames.push((frame.text.expose().clone(), frame.end)),
            WireEvent::Ack | WireEvent::Nak => {}
        }
    }
    (!frames.is_empty()).then_some(frames)
}

/// The sample of an AU message from its wire frames, if they hold a complete one.
fn au_results(config: &AuConfig, original: &[u8]) -> Option<SampleResults> {
    let mut decoder = StreamDecoder::new(config.clone()).ok()?;
    let mut assembler = ResultAssembler::new();
    for event in decoder.push(original) {
        let Ok(WireEvent::Frame(frame)) = event else {
            continue;
        };
        let Ok(AuMessage::ResultBlock(block)) = packie_au::parse_message(config, &frame) else {
            continue;
        };
        if let Ok(AssemblyOutcome::Complete(results)) = assembler.push(block) {
            return Some(*results);
        }
    }
    None
}

/// The identifiers of a message a bridge tap observed (framed by the tap's framer).
pub(crate) fn tap_identifiers(protocol: ProtocolKind, bytes: &[u8]) -> Identifiers {
    match protocol {
        ProtocolKind::Hl7 => hl7::verdict(bytes)
            .map(|m| hl7::identifiers(&m))
            .unwrap_or_default(),
        ProtocolKind::Astm => astm::identifiers(bytes, astm::STANDARD_SAMPLE).unwrap_or_default(),
        ProtocolKind::Au => Identifiers {
            message_type: Some("au".into()),
            ..Identifiers::default()
        },
        ProtocolKind::Delimited | ProtocolKind::Raw => Identifiers::default(),
    }
}
