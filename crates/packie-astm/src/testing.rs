//! Helpers shared by the receive and link driver tests.

use proptest::prelude::*;

use crate::contract::{Action, Event};
use crate::frame::{ACK, CR, ENQ, EOT, FrameEnd, encode_frame};

/// ENQ, one final frame holding `record`, EOT: the bytes of one complete inbound transfer.
pub(crate) fn transfer_bytes(record: &[u8]) -> Vec<u8> {
    let frame = encode_frame(1, &[record, &[CR]].concat(), FrameEnd::Final).expect("frame encodes");
    [vec![ENQ], frame, vec![EOT]].concat()
}

/// Cuts `stream` at the given positions (each reduced into range) into ordered chunks.
pub(crate) fn chunked<'a>(stream: &'a [u8], cuts: &[prop::sample::Index]) -> Vec<&'a [u8]> {
    let mut points: Vec<usize> = cuts.iter().map(|cut| cut.index(stream.len() + 1)).collect();
    points.sort_unstable();
    let mut chunks = Vec::new();
    let mut start = 0;
    for point in points {
        chunks.push(&stream[start..point]);
        start = point;
    }
    chunks.push(&stream[start..]);
    chunks
}

/// The event that answers `action`: the frame it asks to persist is confirmed at once.
fn confirmation(action: &Action) -> Option<Event> {
    let Action::PersistFrame { frame_id, .. } = action else {
        return None;
    };
    Some(Event::Persisted(*frame_id))
}

fn message(action: &Action) -> Option<Vec<Vec<u8>>> {
    let Action::MessageReceived(records) = action else {
        return None;
    };
    Some(records.clone())
}

/// Feeds `chunks` in order to `handle`, confirming every persisted frame at once. Returns
/// the inbound messages and the number of ACKs written.
pub(crate) fn receive_chunks(
    chunks: &[&[u8]],
    mut handle: impl FnMut(Event) -> Vec<Action>,
) -> (Vec<Vec<Vec<u8>>>, usize) {
    let mut messages = Vec::new();
    let mut ack_count = 0;
    for chunk in chunks {
        let mut pending = vec![Event::Bytes(chunk.to_vec())];
        while let Some(event) = pending.pop() {
            let actions = handle(event);
            ack_count += actions
                .iter()
                .filter(|action| **action == Action::Write(vec![ACK]))
                .count();
            messages.extend(actions.iter().filter_map(message));
            pending.extend(actions.iter().filter_map(confirmation));
        }
    }
    (messages, ack_count)
}
