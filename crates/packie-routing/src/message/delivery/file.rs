//! Delimited-file destinations: one file per delivery (`docs/spec/message-channels.md`
//! §8).

use std::time::Duration;

use packie_config::{Destination, ProtocolSettings};
use packie_core::{AckLevel, AttemptResult, Timestamp};
use packie_transport::FileSink;

use super::outcome::Outcome;
use crate::message::store::PendingDelivery;
use crate::message::{MessageContext, files};

/// Writes the delivery as one file named by the template's `file_name`; a complete
/// file proves `handed_off` (`message-channels.md` §8).
pub(super) async fn send_file(
    cx: &MessageContext,
    destination: &Destination,
    sink: &dyn FileSink,
    pending: &PendingDelivery,
    body: &[u8],
    limit: Duration,
) -> Outcome {
    let ProtocolSettings::Delimited(settings) = &destination.protocol else {
        return Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true);
    };
    let pattern = settings.file_name.as_deref().unwrap_or("{message_id}.csv");
    let name = files::file_name(pattern, pending, Timestamp::now());
    let written = tokio::time::timeout(limit, sink.write(&name, body, settings.on_exists)).await;
    match written {
        Ok(Ok(written)) => {
            cx.counters_of(destination.endpoint).bytes_out(body.len());
            cx.shared.message_sent(body.len());
            // The name may carry identifiers: the event names the delivery instead.
            let suffixed = written != name;
            tracing::debug!(
                event = "file.written",
                destination_id = %destination.id,
                delivery_id = %pending.delivery_id,
                bytes = body.len(),
                suffixed,
                "Delivery {} written to the destination folder",
                pending.delivery_id
            );
            Outcome::new(AttemptResult::Acknowledged, AckLevel::HandedOff, true)
        }
        Ok(Err(error)) => {
            Outcome::new(AttemptResult::WriteFailed, AckLevel::None, true).cause(format!(
                "the file of delivery {} was not written: {}",
                pending.delivery_id,
                files::without_name(&error.cause, &name)
            ))
        }
        Err(_) => Outcome::new(AttemptResult::SendTimeout, AckLevel::None, true)
            .cause("the file was not written within send_timeout_seconds"),
    }
}
