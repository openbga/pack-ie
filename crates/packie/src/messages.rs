//! `packie messages`: list and inspect stored messages, resend them and close deliveries.

use clap::{Args, Subcommand};
use packie_core::{
    ByteSize, ChannelId, ConnectorId, DeliveryId, DeliveryState, DeliverySummary, MessageDirection,
    MessageId, MessageState, Timestamp,
};
use packie_ipc::messages::{
    BodyFormat, BodyRequest, BodySelector, CloseRequest, IdentifierFilter, MAX_PAGE, MAX_REASON,
    MessageBody, MessageFilter, MessageList, MessageQuery, MessageRecord, MessageSort,
    MessageSortField, ParsedFields, ResendMode, ResendRequest, ResendResult, ResendTargets,
    id_prefix,
};
use packie_ipc::{IpcClient, Request, Response};

use crate::client::{Target, ask, hint, unexpected};
use crate::message_view::{LIST_HEADERS, detail_lines, field_lines, list_row};
use crate::output::{count, print_json, print_line, print_lines, print_table, short_time};
use crate::{Failure, args};

#[derive(Debug, Subcommand)]
pub(crate) enum MessagesCommand {
    /// List stored messages, newest first.
    List(ListArgs),
    /// Show one message with its deliveries and attempts; a body only with --body.
    Show(ShowArgs),
    /// Queue a message for another delivery attempt.
    Resend(ResendArgs),
    /// Close a delivery that needs attention, with a reason. The destination is not contacted.
    Close(CloseArgs),
    /// Export the messages the filters select, oldest first, as CSV or JSON Lines (audited;
    /// bodies only with --bodies).
    Export(crate::reports::ExportArgs),
    /// Report what a channel received and delivered in a period, per destination, with
    /// content hashes (transmission verification; audited).
    Verify(crate::reports::VerifyArgs),
}

/// Which messages: the filters of `list` and `export`.
#[derive(Debug, Args)]
pub(crate) struct FilterArgs {
    /// Only messages of this channel id (repeat or separate with commas for several).
    #[arg(long, value_delimiter = ',')]
    channel: Vec<ChannelId>,
    /// Only messages in this state: receiving, received, routed, filtered, duplicate,
    /// invalid, aborted or observed (several allowed).
    #[arg(long, value_delimiter = ',', value_parser = args::message_state)]
    state: Vec<MessageState>,
    /// Only messages with a delivery in this state: queued, sending, acknowledged,
    /// rejected, timed_out, outcome_unknown, failed or cancelled (several allowed).
    #[arg(long, value_delimiter = ',', value_parser = args::delivery_state)]
    delivery_state: Vec<DeliveryState>,
    /// Only messages whose deliveries need attention (rejected, failed, timed out or
    /// outcome unknown).
    #[arg(long)]
    attention: bool,
    /// Only messages of this direction: inbound, a_to_b or b_to_a (several allowed).
    #[arg(long, value_delimiter = ',', value_parser = args::direction)]
    direction: Vec<MessageDirection>,
    /// Only messages received at or after this time: RFC 3339 (2026-11-06T08:00:00Z) or an
    /// age such as 15m, 2h or 7d.
    #[arg(long, value_parser = args::time)]
    since: Option<Timestamp>,
    /// Only messages received before this time (same forms as --since).
    #[arg(long, value_parser = args::time)]
    until: Option<Timestamp>,
    /// Only the message whose control, sample or patient id is this.
    #[arg(long)]
    identifier: Option<String>,
    /// Match ids that start with --identifier instead of equalling it.
    #[arg(long, requires = "identifier")]
    prefix: bool,
    /// Only messages containing this text (at least 3 characters) in a control, sample or
    /// patient id or the message type (bodies are not searched); narrow it with --since.
    #[arg(long)]
    text: Option<String>,
    /// Only messages whose id starts with these hex digits (at least 8).
    #[arg(long)]
    id: Option<String>,
}

impl FilterArgs {
    pub(crate) fn filter(&self) -> MessageFilter {
        MessageFilter {
            since: self.since,
            until: self.until,
            channels: self.channel.clone(),
            directions: self.direction.clone(),
            states: self.state.clone(),
            delivery_states: self.delivery_state.clone(),
            delivery_summaries: if self.attention {
                vec![DeliverySummary::Attention]
            } else {
                Vec::new()
            },
            identifier: self.identifier.clone().map(|value| IdentifierFilter {
                value,
                prefix: self.prefix,
            }),
            text: self.text.clone(),
            message_id_prefix: self.id.clone(),
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct ListArgs {
    #[command(flatten)]
    filter: FilterArgs,
    /// Oldest first instead of newest first.
    #[arg(long)]
    oldest_first: bool,
    /// Rows to skip.
    #[arg(long, default_value_t = 0)]
    offset: u32,
    /// Rows to print, 1 to 1000.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=i64::from(MAX_PAGE)))]
    limit: u32,
    /// Print the page as JSON.
    #[arg(long)]
    json: bool,
}

impl ListArgs {
    fn query(&self) -> MessageQuery {
        MessageQuery {
            filter: self.filter.filter(),
            sort: MessageSort {
                field: MessageSortField::ReceivedAt,
                descending: !self.oldest_first,
            },
            offset: self.offset,
            limit: self.limit,
        }
    }
}

#[derive(Debug, Args)]
pub(crate) struct ShowArgs {
    /// The message id as `packie messages list` prints it, or its first 8 or more hex digits.
    message: String,
    /// Also print a body: `original` (the bytes as received) or a destination id (what was
    /// sent there). Message content is printed only with this flag.
    #[arg(long, value_parser = body_choice)]
    body: Option<BodyChoice>,
    /// How to render the body: `text` (control characters made visible) or `hex`.
    #[arg(long = "as", default_value = "text", requires = "body", value_parser = body_format)]
    render: BodyFormat,
    /// Also print the original's fields by the paths filters and mappings use (HL7 and
    /// ASTM). Message content is printed only with this flag.
    #[arg(long)]
    fields: bool,
    /// Print the message (and body, and fields) as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BodyChoice {
    Original,
    Destination(ConnectorId),
}

fn body_choice(text: &str) -> Result<BodyChoice, String> {
    if text == "original" {
        return Ok(BodyChoice::Original);
    }
    text.parse()
        .map(BodyChoice::Destination)
        .map_err(|_| format!("`{text}` is neither `original` nor a destination id"))
}

fn body_format(text: &str) -> Result<BodyFormat, String> {
    match text {
        "text" => Ok(BodyFormat::Text),
        "hex" => Ok(BodyFormat::Hex),
        _ => Err(format!("`{text}` is not one of: text, hex")),
    }
}

#[derive(Debug, Args)]
pub(crate) struct ResendArgs {
    /// The message id, or its first 8 or more hex digits.
    message: String,
    /// Resend only to this destination id (repeat or separate with commas for several);
    /// default: every destination of the channel.
    #[arg(long, value_delimiter = ',')]
    destination: Vec<ConnectorId>,
    /// Create a new message from the original bytes and run it through the current
    /// configuration, instead of retrying the existing deliveries with their stored output.
    #[arg(long)]
    new_message: bool,
    /// Resend even where the destination may already have processed the message
    /// (acknowledged, timed out or outcome unknown deliveries).
    #[arg(long)]
    confirm_duplicate_risk: bool,
    /// Resend even though a newer revision of the message exists (it may reissue a
    /// corrected result).
    #[arg(long)]
    superseded_ok: bool,
    /// Why, shown with the new attempt cycle (1 to 500 characters).
    #[arg(long)]
    reason: Option<String>,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
pub(crate) struct CloseArgs {
    /// The delivery id (shown by `packie messages show`).
    delivery: DeliveryId,
    /// Why the delivery is closed, 1 to 500 characters; kept with the delivery.
    #[arg(long)]
    reason: String,
    /// Print the closed delivery as JSON.
    #[arg(long)]
    json: bool,
}

pub(crate) async fn run(target: &Target, command: MessagesCommand) -> Result<(), Failure> {
    match command {
        MessagesCommand::List(args) => list(target, &args).await,
        MessagesCommand::Show(args) => show(target, &args).await,
        MessagesCommand::Resend(args) => resend(target, &args).await,
        MessagesCommand::Close(args) => close(target, &args).await,
        MessagesCommand::Export(args) => crate::reports::export(target, &args).await,
        MessagesCommand::Verify(args) => crate::reports::verify(target, &args).await,
    }
}

async fn list(target: &Target, args: &ListArgs) -> Result<(), Failure> {
    let query = args.query();
    let filtered = query.filter != MessageFilter::default();
    let client = target.connect().await?;
    let Response::Messages(page) = ask(&client, Request::GetMessages(Box::new(query))).await?
    else {
        return Err(unexpected());
    };
    if args.json {
        return print_json(&page);
    }
    if page.messages.is_empty() {
        print_line(&empty_text(&page, filtered));
        return Ok(());
    }
    let rows: Vec<Vec<String>> = page.messages.iter().map(list_row).collect();
    print_table(&LIST_HEADERS, &rows);
    print_line("");
    let shown = u64::try_from(page.messages.len()).unwrap_or(u64::MAX);
    print_line(&page_footer(page.offset, shown, page.total));
    Ok(())
}

/// What an empty page means: nothing stored, nothing matching, or a page past the end.
fn empty_text(page: &MessageList, filtered: bool) -> String {
    match (page.total, filtered) {
        (0, false) => {
            "No messages stored yet. A message appears here once a channel receives one.".to_owned()
        }
        (0, true) => {
            "No messages match these filters. Widen them, or run without filters.".to_owned()
        }
        (total, _) => format!(
            "Offset {} is past the last of {}. Use a smaller --offset.",
            page.offset,
            count(total, "matching message", "matching messages")
        ),
    }
}

/// `Showing 1-20 of 57 messages. Next page: --offset 20.`
fn page_footer(offset: u32, shown: u64, total: u64) -> String {
    let first = u64::from(offset).saturating_add(1);
    let last = u64::from(offset).saturating_add(shown);
    let showing = format!(
        "Showing {first}-{last} of {}.",
        count(total, "message", "messages")
    );
    if last < total {
        format!("{showing} Next page: --offset {last}.")
    } else {
        showing
    }
}

async fn show(target: &Target, args: &ShowArgs) -> Result<(), Failure> {
    let client = target.connect().await?;
    let id = resolve(&client, &args.message).await?;
    let record = message(&client, id).await?;
    let body = match &args.body {
        Some(choice) => Some(body(&client, &record, choice, args.render).await?),
        None => None,
    };
    // Content is fetched only when asked for; the engine audits the view.
    let parsed = if args.fields {
        Some(fields(&client, id).await?)
    } else {
        None
    };
    if args.json {
        return print_json(&record_json(&record, body.as_ref(), parsed.as_ref())?);
    }
    print_lines(&detail_lines(&record));
    if args.fields {
        print_line("");
        print_lines(&field_lines(parsed.as_ref()));
    }
    if let Some(body) = body {
        print_line("");
        print_line(&body_heading(&body));
        print_line(&body.content);
    }
    Ok(())
}

/// The record as JSON, with the body and the parsed fields added under `body` and `fields`
/// when they were asked for.
fn record_json(
    record: &MessageRecord,
    body: Option<&MessageBody>,
    fields: Option<&ParsedFields>,
) -> Result<serde_json::Value, Failure> {
    let as_failure = |e: serde_json::Error| Failure::new(e.to_string());
    let mut json = serde_json::to_value(record).map_err(as_failure)?;
    if let Some(object) = json.as_object_mut() {
        if let Some(body) = body {
            object.insert(
                "body".to_owned(),
                serde_json::to_value(body).map_err(as_failure)?,
            );
        }
        if let Some(fields) = fields {
            object.insert(
                "fields".to_owned(),
                serde_json::to_value(fields).map_err(as_failure)?,
            );
        }
    }
    Ok(json)
}

fn body_heading(body: &MessageBody) -> String {
    let which = match body.which {
        BodySelector::Original => "original".to_owned(),
        BodySelector::Transformed { delivery_id } => format!("sent with delivery {delivery_id}"),
    };
    let truncated = if body.truncated {
        " (shown truncated at the display limit)"
    } else {
        ""
    };
    format!(
        "Body: {which}, {}, sha256 {}{truncated}",
        ByteSize(body.byte_length),
        body.hash
    )
}

/// A full message id, or the one message whose id starts with `text`.
async fn resolve(client: &IpcClient, text: &str) -> Result<MessageId, Failure> {
    if let Ok(id) = text.trim().parse::<MessageId>() {
        return Ok(id);
    }
    let prefix = id_prefix(text).map_err(|e| Failure::new(format!("`{text}`: {e}.")))?;
    let query = MessageQuery {
        filter: MessageFilter {
            message_id_prefix: Some(prefix.clone()),
            ..MessageFilter::default()
        },
        limit: 10,
        ..MessageQuery::default()
    };
    let Response::Messages(list) = ask(client, Request::GetMessages(Box::new(query))).await? else {
        return Err(unexpected());
    };
    let found: Vec<MessageId> = list
        .messages
        .iter()
        .map(|m| m.message_id)
        .filter(|id| id.to_string().starts_with(&prefix))
        .collect();
    match found.as_slice() {
        [id] => Ok(*id),
        [] => Err(Failure::new(format!(
            "No stored message id starts with {prefix}. `packie messages list` shows the ids."
        ))),
        several => Err(Failure::new(format!(
            "{prefix} matches {} messages ({}); give more digits.",
            several.len(),
            several
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

async fn fields(client: &IpcClient, id: MessageId) -> Result<ParsedFields, Failure> {
    match ask(client, Request::GetMessageFields { message_id: id }).await? {
        Response::MessageFields(fields) => Ok(*fields),
        _ => Err(unexpected()),
    }
}

async fn message(client: &IpcClient, id: MessageId) -> Result<MessageRecord, Failure> {
    match ask(client, Request::GetMessage { message_id: id }).await? {
        Response::Message { message } => Ok(*message),
        _ => Err(unexpected()),
    }
}

async fn body(
    client: &IpcClient,
    record: &MessageRecord,
    choice: &BodyChoice,
    format: BodyFormat,
) -> Result<MessageBody, Failure> {
    let which = match choice {
        BodyChoice::Original => BodySelector::Original,
        BodyChoice::Destination(destination) => {
            let delivery = record
                .deliveries
                .iter()
                .find(|d| d.destination_id == *destination)
                .ok_or_else(|| no_such_destination(record, destination))?;
            BodySelector::Transformed {
                delivery_id: delivery.delivery_id,
            }
        }
    };
    let request = BodyRequest {
        message_id: record.summary.message_id,
        which,
        format,
    };
    match ask(client, Request::GetMessageBody(request)).await? {
        Response::MessageBody(body) => Ok(body),
        _ => Err(unexpected()),
    }
}

fn no_such_destination(record: &MessageRecord, destination: &ConnectorId) -> Failure {
    let known: Vec<String> = record
        .deliveries
        .iter()
        .map(|d| d.destination_id.to_string())
        .collect();
    let known = if known.is_empty() {
        "it has no deliveries".to_owned()
    } else {
        format!("its destinations are: {}", known.join(", "))
    };
    Failure::new(format!(
        "message {} has no delivery to destination {destination}; {known}. Use --body original for the bytes as received.",
        record.summary.message_id
    ))
}

async fn resend(target: &Target, args: &ResendArgs) -> Result<(), Failure> {
    let client = target.connect().await?;
    let message_id = resolve(&client, &args.message).await?;
    let request = ResendRequest {
        message_id,
        destinations: if args.destination.is_empty() {
            ResendTargets::All
        } else {
            ResendTargets::Destinations(args.destination.clone())
        },
        mode: if args.new_message {
            ResendMode::NewMessage
        } else {
            ResendMode::SameMessage
        },
        confirm_duplicate_risk: args.confirm_duplicate_risk,
        reason: args.reason.clone(),
        confirm_superseded: args.superseded_ok,
    };
    let Response::Resent(result) = ask(&client, Request::Resend(request)).await? else {
        return Err(unexpected());
    };
    if args.json {
        print_json(&result)?;
    } else {
        print_resend(message_id, &result);
    }
    resend_verdict(message_id, &result)
}

fn print_resend(message: MessageId, result: &ResendResult) {
    if let Some(new_id) = result.new_message_id {
        print_line(&format!(
            "Created message {new_id} from the original bytes of {message}; it runs through the current configuration."
        ));
    }
    let rows: Vec<Vec<String>> = result
        .destinations
        .iter()
        .map(|d| {
            let outcome = match (&d.delivery_id, &d.rejected) {
                (Some(id), _) => format!("queued delivery {id}"),
                (None, Some(rejection)) => format!("rejected: {}", rejection.message),
                (None, None) => "accepted".to_owned(),
            };
            vec![d.destination_id.to_string(), outcome]
        })
        .collect();
    print_table(&["DESTINATION", "RESULT"], &rows);
    if let Some(new_id) = result.new_message_id {
        print_line(&format!(
            "Check how the new message went with `packie messages show {new_id}`."
        ));
        return;
    }
    let queued = result
        .destinations
        .iter()
        .filter(|d| d.delivery_id.is_some())
        .count();
    if queued > 0 {
        let queued = u64::try_from(queued).unwrap_or(u64::MAX);
        print_line(&format!(
            "Queued {}. Check the outcome with `packie messages show {message}`.",
            count(queued, "delivery", "deliveries")
        ));
    }
}

/// The command fails when any requested destination was rejected, and says what to do.
fn resend_verdict(message: MessageId, result: &ResendResult) -> Result<(), Failure> {
    let rejections: Vec<_> = result
        .destinations
        .iter()
        .filter_map(|d| d.rejected.as_ref())
        .collect();
    if rejections.is_empty() {
        return Ok(());
    }
    let mut text = format!(
        "{} of {} destinations rejected the resend of {message}; nothing was queued for them.",
        rejections.len(),
        result.destinations.len()
    );
    let mut hints: Vec<&str> = Vec::new();
    for hint in rejections.iter().filter_map(|r| hint(r.code)) {
        if !hints.contains(&hint) {
            hints.push(hint);
        }
    }
    for hint in hints {
        text.push(' ');
        text.push_str(hint);
    }
    Err(Failure::new(text))
}

async fn close(target: &Target, args: &CloseArgs) -> Result<(), Failure> {
    let reason = args.reason.trim();
    if reason.is_empty() || reason.chars().count() > MAX_REASON {
        return Err(Failure::new(format!(
            "--reason must be 1 to {MAX_REASON} characters"
        )));
    }
    let client = target.connect().await?;
    let request = CloseRequest {
        delivery_id: args.delivery,
        reason: reason.to_owned(),
    };
    let Response::DeliveryClosed { delivery } =
        ask(&client, Request::CloseDelivery(request)).await?
    else {
        return Err(unexpected());
    };
    if args.json {
        return print_json(&delivery);
    }
    print_line(&format!(
        "Closed delivery {} of message {} to {} at {}. The destination was not contacted and no further attempt will be made.",
        delivery.delivery_id,
        delivery.message_id,
        delivery.destination_id,
        short_time(delivery.updated_at),
    ));
    print_line(&format!(
        "To deliver it after all, run `packie messages resend {} --destination {}`.",
        delivery.message_id, delivery.destination_id
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use packie_core::MessageId;
    use packie_ipc::messages::{
        MessageList, Rejection, RejectionCode, ResendOutcome, ResendResult,
    };

    use super::{BodyChoice, body_choice, empty_text, page_footer, resend_verdict};

    fn page(total: u64, offset: u32) -> MessageList {
        MessageList {
            messages: Vec::new(),
            total,
            offset,
            limit: 20,
        }
    }

    #[test]
    fn empty_pages_explain_themselves() {
        assert!(empty_text(&page(0, 0), false).starts_with("No messages stored yet"));
        assert!(empty_text(&page(0, 0), true).starts_with("No messages match"));
        assert_eq!(
            empty_text(&page(5, 40), true),
            "Offset 40 is past the last of 5 matching messages. Use a smaller --offset."
        );
    }

    #[test]
    fn a_footer_points_to_the_next_page_only_when_there_is_one() {
        assert_eq!(
            page_footer(0, 20, 57),
            "Showing 1-20 of 57 messages. Next page: --offset 20."
        );
        assert_eq!(page_footer(40, 17, 57), "Showing 41-57 of 57 messages.");
        assert_eq!(page_footer(0, 1, 1), "Showing 1-1 of 1 message.");
    }

    #[test]
    fn bodies_are_original_or_a_destination() {
        assert_eq!(body_choice("original"), Ok(BodyChoice::Original));
        assert_eq!(
            body_choice("lis"),
            Ok(BodyChoice::Destination("lis".parse().unwrap()))
        );
        assert!(body_choice("Not A Name").is_err());
    }

    #[test]
    fn a_resend_with_a_rejected_destination_fails_with_the_remedy() {
        let rejected = |code| ResendOutcome {
            destination_id: "lis".parse().unwrap(),
            destination_label: "LIS".to_owned(),
            delivery_id: None,
            rejected: Some(Rejection {
                code,
                message: "may already have been processed".to_owned(),
            }),
        };
        let result = ResendResult {
            new_message_id: None,
            destinations: vec![
                rejected(RejectionCode::DuplicateRiskUnconfirmed),
                rejected(RejectionCode::DuplicateRiskUnconfirmed),
            ],
        };
        let error = resend_verdict(MessageId::generate(), &result).unwrap_err();
        assert!(
            error.message.contains("2 of 2 destinations rejected"),
            "{}",
            error.message
        );
        assert_eq!(
            error.message.matches("--confirm-duplicate-risk").count(),
            1,
            "{}",
            error.message
        );

        let queued = ResendResult {
            new_message_id: None,
            destinations: Vec::new(),
        };
        assert!(resend_verdict(MessageId::generate(), &queued).is_ok());
    }
}
