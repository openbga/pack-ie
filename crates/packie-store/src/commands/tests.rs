use packie_core::{
    AckLevel, AttemptResult, ConnectorId, DeliveryState, MessageId, MessageOrigin, MessageState,
    Timestamp,
};

use super::*;
use crate::testing::{completion, fixture};

fn request(message_id: MessageId, mode: ResendMode) -> ResendRequest {
    ResendRequest {
        message_id,
        destinations: ResendTargets::All,
        mode,
        confirm_duplicate_risk: false,
        reason: None,
        confirm_superseded: false,
    }
}

#[tokio::test]
async fn same_resend_preserves_body_and_history_and_requires_risk_confirmation() {
    let (_directory, store, receipt, destination) = fixture();
    let outcome = store
        .commit_received(receipt, b"original".to_vec())
        .await
        .unwrap();
    let id = outcome.deliveries[0];
    store
        .record_transformed_body(id, b"frozen".to_vec(), 7)
        .await
        .unwrap();
    store.begin_attempt(id, Timestamp::now()).await.unwrap();
    assert!(matches!(
        store
            .close_delivery(
                CloseRequest {
                    delivery_id: id,
                    reason: "Checked".into()
                },
                Timestamp::now()
            )
            .await,
        Err(Error::Rejected {
            code: RejectionCode::DeliveryInProgress,
            ..
        })
    ));
    store
        .finish_attempt(
            id,
            completion(AttemptResult::Acknowledged, AckLevel::Accepted),
        )
        .await
        .unwrap();
    let mut command = request(outcome.message_id, ResendMode::SameMessage);
    let refused = store
        .resend(
            command.clone(),
            vec![destination.clone()],
            8,
            Timestamp::now(),
        )
        .await
        .unwrap();
    assert_eq!(
        refused.destinations[0].rejected.as_ref().unwrap().code,
        RejectionCode::DuplicateRiskUnconfirmed
    );
    command.confirm_duplicate_risk = true;
    store
        .resend(command, vec![destination], 8, Timestamp::now())
        .await
        .unwrap();
    let record = store.delivery(id).unwrap();
    assert_eq!(
        (
            record.attempts,
            record.total_attempts,
            record.mapping_version
        ),
        (0, 1, Some(7))
    );
    assert_eq!(store.body(outcome.message_id, Some(id)).unwrap(), b"frozen");
    assert!(
        store
            .record_transformed_body(id, b"new".to_vec(), 8)
            .await
            .is_err()
    );
    assert_eq!(store.begin_attempt(id, Timestamp::now()).await.unwrap(), 2);
    assert_eq!(store.delivery(id).unwrap().attempt_log.len(), 2);
}

#[tokio::test]
async fn new_resend_copies_refs_and_waits_for_selected_pipeline() {
    let (_directory, store, receipt, destination) = fixture();
    let original = store
        .commit_received(receipt, b"original".to_vec())
        .await
        .unwrap();
    let absent = ConnectorId::generate();
    let mut command = request(original.message_id, ResendMode::NewMessage);
    command.destinations =
        ResendTargets::Destinations(vec![destination.id.parse().unwrap(), absent]);
    let result = store
        .resend(command, vec![destination.clone()], 9, Timestamp::now())
        .await
        .unwrap();
    assert!(result.destinations[0].rejected.is_none());
    assert!(result.destinations[0].delivery_id.is_none());
    assert_eq!(
        result.destinations[1].rejected.as_ref().unwrap().code,
        RejectionCode::DestinationNotFound
    );
    let id = result.new_message_id.unwrap();
    let record = store.message(id).unwrap();
    assert_eq!(record.summary.state, MessageState::Received);
    assert_eq!(record.summary.origin, MessageOrigin::Resend);
    assert_eq!(record.summary.resent_from, Some(original.message_id));
    assert!(record.deliveries.is_empty());
    assert_eq!(store.resend_targets(id).unwrap(), vec![destination.id]);
    assert_eq!(store.body(id, None).unwrap(), b"original");
    assert_eq!(
        store.delivery(original.deliveries[0]).unwrap().state,
        DeliveryState::Queued
    );
}

#[tokio::test]
async fn incomplete_and_observed_receipts_cannot_be_resent_through_pipeline() {
    let (_directory, store, mut receipt, destination) = fixture();
    receipt.destinations.clear();
    for state in [MessageState::Receiving, MessageState::Observed] {
        receipt.initial_state = state;
        let outcome = store
            .commit_received(receipt.clone(), vec![1])
            .await
            .unwrap();
        let result = store
            .resend(
                request(outcome.message_id, ResendMode::NewMessage),
                vec![destination.clone()],
                2,
                Timestamp::now(),
            )
            .await
            .unwrap();
        assert!(result.new_message_id.is_none());
        assert_eq!(
            result.destinations[0].rejected.as_ref().unwrap().code,
            RejectionCode::InvalidState
        );
    }
}

#[tokio::test]
async fn bodyless_resend_requires_mapping_before_attempt() {
    let (_directory, store, mut receipt, destination) = fixture();
    receipt.destinations[0].transform_error = Some("required mapping field missing".into());
    let outcome = store
        .commit_received(receipt, b"original".to_vec())
        .await
        .unwrap();
    let id = outcome.deliveries[0];
    let command = request(outcome.message_id, ResendMode::SameMessage);
    store
        .resend(
            command.clone(),
            vec![destination.clone()],
            8,
            Timestamp::now(),
        )
        .await
        .unwrap();
    assert_eq!(
        store.delivery(id).unwrap().reason_code.as_deref(),
        Some("mapping_required")
    );
    assert!(store.begin_attempt(id, Timestamp::now()).await.is_err());
    store
        .record_transform_error(
            id,
            "mapping still cannot encode".into(),
            8,
            Timestamp::now(),
        )
        .await
        .unwrap();
    let record = store.delivery(id).unwrap();
    assert_eq!(record.state, DeliveryState::Failed);
    assert_eq!(record.attempts, 0);
    assert!(record.attempt_log.is_empty());
    store
        .resend(command, vec![destination], 9, Timestamp::now())
        .await
        .unwrap();
    assert!(store.begin_attempt(id, Timestamp::now()).await.is_err());
    store
        .record_transformed_body(id, b"mapped".to_vec(), 9)
        .await
        .unwrap();
    let record = store.delivery(id).unwrap();
    assert!(record.reason_code.is_none());
    assert_eq!(record.mapping_version, Some(9));
    store.begin_attempt(id, Timestamp::now()).await.unwrap();
    assert!(
        store
            .record_transform_error(id, "late mapping".into(), 10, Timestamp::now())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn closing_transform_failure_does_not_bypass_mapping_on_resend() {
    let (_directory, store, receipt, destination) = fixture();
    let outcome = store
        .commit_received(receipt, b"original".to_vec())
        .await
        .unwrap();
    let id = outcome.deliveries[0];
    store
        .record_transform_error(id, "mapping cannot encode".into(), 1, Timestamp::now())
        .await
        .unwrap();
    store
        .close_delivery(
            CloseRequest {
                delivery_id: id,
                reason: "Closed while fixing mapping".into(),
            },
            Timestamp::now(),
        )
        .await
        .unwrap();
    store
        .resend(
            request(outcome.message_id, ResendMode::SameMessage),
            vec![destination],
            2,
            Timestamp::now(),
        )
        .await
        .unwrap();
    let record = store.delivery(id).unwrap();
    assert_eq!(record.reason_code.as_deref(), Some("mapping_required"));
    assert!(record.closed_reason.is_none());
    assert!(store.begin_attempt(id, Timestamp::now()).await.is_err());
    store
        .record_transformed_body(id, b"mapped".to_vec(), 2)
        .await
        .unwrap();
    assert_eq!(store.begin_attempt(id, Timestamp::now()).await.unwrap(), 1);
}

#[tokio::test]
async fn close_validates_reason_and_resolved_state() {
    let (_directory, store, receipt, _) = fixture();
    let id = store
        .commit_received(receipt, vec![1])
        .await
        .unwrap()
        .deliveries[0];
    let close = |reason: &str| CloseRequest {
        delivery_id: id,
        reason: reason.into(),
    };
    assert!(matches!(
        store.close_delivery(close("  "), Timestamp::now()).await,
        Err(Error::Rejected {
            code: RejectionCode::ReasonRequired,
            ..
        })
    ));
    let closed = store
        .close_delivery(close("  Confirmed locally  "), Timestamp::now())
        .await
        .unwrap();
    assert_eq!(closed.state, DeliveryState::Cancelled);
    assert_eq!(closed.closed_reason.as_deref(), Some("Confirmed locally"));
    assert!(matches!(
        store.close_delivery(close("Again"), Timestamp::now()).await,
        Err(Error::Rejected {
            code: RejectionCode::InvalidState,
            ..
        })
    ));
}
