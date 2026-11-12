//! Fixtures shared by the crate's unit tests.

use packie_core::{
    AckLevel, AttemptResult, ChannelId, ConnectorId, EndpointId, MessageDirection, MessageOrigin,
    MessageState, ProtocolKind, Timestamp,
};
use tempfile::TempDir;

use crate::{AttemptFinish, Destination, Identifiers, OpenOptions, Receipt, Store};

/// A store in a fresh state directory, removed when the guard drops.
pub(crate) fn opened() -> (TempDir, Store) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path(), OpenOptions::default()).unwrap();
    (directory, store)
}

/// A received HL7 message for a new channel, without destinations.
pub(crate) fn receipt() -> Receipt {
    Receipt {
        channel_id: ChannelId::generate().to_string(),
        channel_label: "Channel".into(),
        source_connector_id: ConnectorId::generate().to_string(),
        source_endpoint_id: EndpointId::generate().to_string(),
        source_peer: None,
        source_file: None,
        protocol: ProtocolKind::Hl7,
        direction: MessageDirection::Inbound,
        origin: MessageOrigin::Received,
        initial_state: MessageState::Received,
        identifiers: Identifiers::default(),
        received_at: Timestamp::now(),
        config_version: 1,
        duplicate_detection: true,
        destinations: Vec::new(),
    }
}

/// An open store and a receipt routed to one default destination (also returned).
pub(crate) fn fixture() -> (TempDir, Store, Receipt, Destination) {
    let (directory, store) = opened();
    let destination = Destination::new(ConnectorId::generate(), "Destination");
    let receipt = Receipt {
        duplicate_detection: false,
        destinations: vec![destination.clone()],
        ..receipt()
    };
    (directory, store, receipt, destination)
}

/// An attempt that ended now with `result`, no retry time and no codes.
pub(crate) fn completion(result: AttemptResult, ack_level: AckLevel) -> AttemptFinish {
    AttemptFinish {
        result,
        ack_level,
        ack_code: None,
        ack_control_id: None,
        error_code: None,
        error_summary: None,
        ended_at: Timestamp::now(),
        retry_at: None,
    }
}
