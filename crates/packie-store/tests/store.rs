#![cfg(test)]

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use packie_core::{
    AckLevel, AttemptResult, ChannelId, ConnectorId, DeliveryState, EndpointId, MessageDirection,
    MessageId, MessageOrigin, MessageState, ProtocolKind, Timestamp,
};
use packie_ipc::messages::{
    CloseRequest, IdentifierFilter, MessageQuery, MessageSort, MessageSortField, RejectionCode,
    ResendMode, ResendRequest, ResendTargets,
};
use packie_store::{
    Alert, AlertFilter, AttemptFinish, Destination, Identifiers, OpenOptions, Receipt,
    ReceiptOutcome, Retention, Store,
};
use rusqlite::{Connection, params};
use tempfile::TempDir;

fn opened() -> (TempDir, Store) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path(), OpenOptions::default()).unwrap();
    (directory, store)
}

fn at(second: i64) -> Timestamp {
    Timestamp::from_unix_millis(1_793_966_400_000 + second * 1000).unwrap()
}

fn receipt() -> Receipt {
    Receipt {
        channel_id: ChannelId::generate().to_string(),
        channel_label: "Analyzer results".into(),
        source_connector_id: ConnectorId::generate().to_string(),
        source_endpoint_id: EndpointId::generate().to_string(),
        source_peer: Some("127.0.0.1:5000".into()),
        source_file: None,
        protocol: ProtocolKind::Hl7,
        direction: MessageDirection::Inbound,
        origin: MessageOrigin::Received,
        initial_state: MessageState::Received,
        identifiers: Identifiers::default(),
        received_at: at(0),
        config_version: 1,
        duplicate_detection: true,
        destinations: Vec::new(),
    }
}

fn destination() -> Destination {
    Destination::new(ConnectorId::generate(), "Results destination")
}

fn finish(result: AttemptResult, ack_level: AckLevel) -> AttemptFinish {
    AttemptFinish {
        result,
        ack_level,
        ack_code: None,
        ack_control_id: None,
        error_code: None,
        error_summary: None,
        ended_at: at(20),
        retry_at: Some(at(30)),
    }
}

async fn commit(store: &Store, receipt: Receipt, bytes: impl Into<Vec<u8>>) -> ReceiptOutcome {
    store.commit_received(receipt, bytes.into()).await.unwrap()
}

fn resend_request(message_id: MessageId, mode: ResendMode) -> ResendRequest {
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
async fn receipt_is_visible_to_an_independent_connection_on_return_and_survives_reopen() {
    let (directory, store) = opened();
    let receipt = receipt();
    let outcome = commit(&store, receipt.clone(), b"original\r\0\xff").await;
    let independent = Connection::open(store.database_path()).unwrap();
    let row: (String, Vec<u8>) = independent.query_row(
        "SELECT m.state, c.inline_bytes FROM messages m JOIN message_bodies b ON b.message_id=m.id JOIN body_contents c USING(content_hash) WHERE m.id=?",
        [outcome.message_id], |row| Ok((row.get(0)?, row.get(1)?)),
    ).unwrap();
    assert_eq!(row.0, "received");
    assert_eq!(row.1, b"original\r\0\xff");
    assert_eq!(store.body(outcome.message_id, None).unwrap(), row.1);
    drop(independent);
    drop(store);
    let reopened = Store::open(directory.path(), OpenOptions::default()).unwrap();
    let record = reopened.message(outcome.message_id).unwrap();
    assert_eq!(record.summary.seq, 1);
    assert_eq!(record.summary.config_version, 1);
    assert_eq!(record.summary.channel_id.to_string(), receipt.channel_id);
    assert_eq!(reopened.body(outcome.message_id, None).unwrap(), row.1);
    assert_eq!(commit(&reopened, receipt, b"next").await.sequence, 2);
}

#[tokio::test]
async fn duplicate_matching_requires_identity_hash_channel_and_an_eligible_original() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    receipt.identifiers.source_identity = Some("sender|facility|control".into());
    receipt.destinations = vec![destination()];
    let original = commit(&store, receipt.clone(), b"first").await;
    let duplicate = commit(&store, receipt.clone(), b"first").await;
    assert_eq!(duplicate.duplicate_of, Some(original.message_id));
    assert!(duplicate.deliveries.is_empty());
    assert_eq!(
        store.message(duplicate.message_id).unwrap().summary.state,
        MessageState::Duplicate
    );
    let revision = commit(&store, receipt.clone(), b"revised").await;
    assert_eq!(revision.revision_of, Some(original.message_id));
    assert!(revision.duplicate_of.is_none());
    let newest = commit(&store, receipt.clone(), b"revised again").await;
    assert_eq!(newest.revision_of, Some(revision.message_id));
    // Each earlier version names the newest of the chain; the newest names none.
    for older in [original.message_id, revision.message_id] {
        assert_eq!(
            store.message(older).unwrap().revised_by,
            Some(newest.message_id)
        );
    }
    assert_eq!(store.message(newest.message_id).unwrap().revised_by, None);
    let mut other = receipt.clone();
    other.channel_id = ChannelId::generate().to_string();
    assert!(commit(&store, other, b"first").await.duplicate_of.is_none());
    let mut invalid = receipt.clone();
    invalid.destinations.clear();
    invalid.initial_state = MessageState::Invalid;
    invalid.identifiers.source_identity = Some("invalid|only".into());
    let rejected = commit(&store, invalid.clone(), b"invalid").await;
    invalid.initial_state = MessageState::Received;
    let retried = commit(&store, invalid, b"invalid").await;
    assert_ne!(rejected.message_id, retried.message_id);
    assert!(retried.duplicate_of.is_none());
    assert!(retried.revision_of.is_none());
    for origin in [MessageOrigin::Resend, MessageOrigin::Tap] {
        let mut excluded = receipt.clone();
        excluded.origin = origin;
        assert!(
            commit(&store, excluded, b"first")
                .await
                .duplicate_of
                .is_none()
        );
    }
    receipt.duplicate_detection = false;
    assert!(
        commit(&store, receipt, b"first")
            .await
            .duplicate_of
            .is_none()
    );
}

#[tokio::test]
async fn identical_bytes_without_a_meaningful_source_identity_are_never_duplicates() {
    let (_directory, store) = opened();
    let receipt = receipt();
    for identity in [None, Some("||| ".into())] {
        let mut anonymous = receipt.clone();
        anonymous.identifiers.source_identity = identity;
        for _ in 0..2 {
            assert!(
                commit(&store, anonymous.clone(), b"same bytes")
                    .await
                    .duplicate_of
                    .is_none()
            );
        }
    }
}

#[tokio::test]
async fn body_cap_preserves_exact_bytes_and_spills_share_one_private_file() {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(
        directory.path(),
        OpenOptions {
            inline_body_cap: 4,
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let receipt = receipt();
    let inline = commit(&store, receipt.clone(), vec![0, 1, 2, 255]).await;
    let bytes = vec![0, 1, 2, 3, 255];
    let spilled = commit(&store, receipt.clone(), bytes.clone()).await;
    let shared = commit(&store, receipt, bytes.clone()).await;
    let conn = Connection::open(store.database_path()).unwrap();
    let inline_storage: (Option<Vec<u8>>, Option<String>) = conn
        .query_row(
            "SELECT inline_bytes,file_path FROM message_bodies JOIN body_contents USING(content_hash) WHERE message_id=?",
            [inline.message_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(inline_storage, (Some(vec![0, 1, 2, 255]), None));
    let path: String = conn
        .query_row(
            "SELECT file_path FROM message_bodies JOIN body_contents USING(content_hash) WHERE message_id=?",
            [spilled.message_id],
            |row| row.get(0),
        )
        .unwrap();
    let shared_path: String = conn
        .query_row(
            "SELECT file_path FROM message_bodies JOIN body_contents USING(content_hash) WHERE message_id=?",
            [shared.message_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(path, shared_path);
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM body_contents", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        2
    );
    assert_eq!(store.body(spilled.message_id, None).unwrap(), bytes);
    let file = directory.path().join("bodies").join(path);
    assert_eq!(std::fs::read(&file).unwrap(), bytes);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for path in [
            directory.path().to_path_buf(),
            directory.path().join("bodies"),
            file.parent().unwrap().to_path_buf(),
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        for path in [store.database_path().to_path_buf(), file] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}

#[tokio::test]
async fn held_commits_hide_receipts_and_keep_the_acknowledgement_future_pending() {
    let directory = tempfile::tempdir().unwrap();
    let (hold, receiver) = tokio::sync::watch::channel(true);
    let store = Store::open(
        directory.path(),
        OpenOptions {
            commits_held: Some(receiver),
            ..OpenOptions::default()
        },
    )
    .unwrap();
    let writer = store.clone();
    let task =
        tokio::spawn(async move { writer.commit_received(receipt(), b"held".to_vec()).await });
    let conn = Connection::open(store.database_path()).unwrap();
    conn.busy_timeout(Duration::ZERO).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;") {
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::DatabaseBusy =>
            {
                break;
            }
            result => result.unwrap(),
        }
        assert!(
            Instant::now() < deadline,
            "writer never reached its held transaction"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(!task.is_finished());
    assert_eq!(
        conn.query_row("SELECT COUNT(*) FROM messages", [], |row| row
            .get::<_, u64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        store
            .query_messages(&MessageQuery::default())
            .unwrap()
            .total,
        0
    );
    hold.send(false).unwrap();
    let committed = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        conn.query_row("SELECT id FROM messages", [], |row| row
            .get::<_, MessageId>(0))
            .unwrap(),
        committed.message_id
    );
}

#[tokio::test]
async fn attempts_retry_only_when_safe_and_keep_current_and_lifetime_counts() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    let mut policy = destination();
    policy.max_attempts = 2;
    receipt.destinations = vec![policy];
    let outcome = commit(&store, receipt, b"message").await;
    let delivery = outcome.deliveries[0];
    assert!(
        store
            .finish_attempt(delivery, finish(AttemptResult::WriteFailed, AckLevel::None))
            .await
            .is_err()
    );
    assert_eq!(store.begin_attempt(delivery, at(10)).await.unwrap(), 1);
    let conn = Connection::open(store.database_path()).unwrap();
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM delivery_attempts WHERE delivery_id=? AND ended_at IS NULL",
            [delivery],
            |row| row.get::<_, u64>(0)
        )
        .unwrap(),
        1
    );
    assert!(store.begin_attempt(delivery, at(11)).await.is_err());
    assert_eq!(
        store
            .finish_attempt(delivery, finish(AttemptResult::WriteFailed, AckLevel::None))
            .await
            .unwrap(),
        DeliveryState::Queued
    );
    assert!(store.begin_attempt(delivery, at(29)).await.is_err());
    assert_eq!(store.begin_attempt(delivery, at(30)).await.unwrap(), 2);
    let mut last = finish(AttemptResult::SendTimeout, AckLevel::None);
    last.ended_at = at(40);
    assert_eq!(
        store.finish_attempt(delivery, last).await.unwrap(),
        DeliveryState::Failed
    );
    let record = store.delivery(delivery).unwrap();
    assert_eq!((record.attempts, record.total_attempts), (2, 2));
    assert_eq!(
        record
            .attempt_log
            .iter()
            .map(|attempt| attempt.result)
            .collect::<Vec<_>>(),
        [
            Some(AttemptResult::WriteFailed),
            Some(AttemptResult::SendTimeout)
        ]
    );
    assert!(store.begin_attempt(delivery, at(50)).await.is_err());
}

#[tokio::test]
async fn missing_acknowledgements_are_explicit_and_retry_requires_opt_in() {
    let (_directory, store) = opened();
    for (result, expected) in [
        (AttemptResult::AckTimeout, DeliveryState::TimedOut),
        (
            AttemptResult::ConnectionLostAfterSend,
            DeliveryState::OutcomeUnknown,
        ),
        (AttemptResult::EngineStopped, DeliveryState::OutcomeUnknown),
    ] {
        for retry in [false, true] {
            check_missing_acknowledgement(&store, result, expected, retry).await;
        }
    }
}

async fn check_missing_acknowledgement(
    store: &Store,
    result: AttemptResult,
    expected: DeliveryState,
    retry: bool,
) {
    let mut receipt = receipt();
    let mut policy = destination();
    policy.retry_unacknowledged = retry;
    policy.max_attempts = 2;
    receipt.destinations = vec![policy];
    let delivery = commit(store, receipt, b"message").await.deliveries[0];
    store.begin_attempt(delivery, at(10)).await.unwrap();
    let state = store
        .finish_attempt(delivery, finish(result, AckLevel::HandedOff))
        .await
        .unwrap();
    assert_eq!(
        state,
        if retry {
            DeliveryState::Queued
        } else {
            expected
        }
    );
    if retry {
        store.begin_attempt(delivery, at(30)).await.unwrap();
        let mut exhausted = finish(result, AckLevel::HandedOff);
        exhausted.ended_at = at(40);
        assert_eq!(
            store.finish_attempt(delivery, exhausted).await.unwrap(),
            expected
        );
    }
}

#[tokio::test]
async fn lower_acknowledgement_is_visible_but_cannot_complete_delivery() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    receipt.destinations = vec![destination()];
    let delivery = commit(&store, receipt, b"message").await.deliveries[0];
    store.begin_attempt(delivery, at(10)).await.unwrap();
    store
        .record_ack_progress(delivery, AckLevel::Received, Some("CA".into()), at(11))
        .await
        .unwrap();
    assert!(
        store
            .finish_attempt(
                delivery,
                finish(AttemptResult::Acknowledged, AckLevel::Received)
            )
            .await
            .is_err()
    );
    assert_eq!(
        store.delivery(delivery).unwrap().state,
        DeliveryState::Sending
    );
    assert!(
        store
            .record_ack_progress(delivery, AckLevel::HandedOff, None, at(12))
            .await
            .is_err()
    );
    assert_eq!(
        store
            .finish_attempt(
                delivery,
                finish(AttemptResult::AckTimeout, AckLevel::Received)
            )
            .await
            .unwrap(),
        DeliveryState::TimedOut
    );
    let record = store.delivery(delivery).unwrap();
    assert_eq!(record.ack_level, AckLevel::Received);
    assert_eq!(record.attempt_log[0].ack_code.as_deref(), Some("CA"));
    assert!(
        store
            .finish_attempt(
                delivery,
                finish(AttemptResult::Acknowledged, AckLevel::Accepted)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn destination_queue_preserves_receipt_order_but_unresolved_work_does_not_block() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    receipt.destinations = vec![destination()];
    let first = commit(&store, receipt.clone(), b"first").await.deliveries[0];
    let second = commit(&store, receipt, b"second").await.deliveries[0];
    assert!(store.begin_attempt(second, at(10)).await.is_err());
    store.begin_attempt(first, at(10)).await.unwrap();
    assert!(store.begin_attempt(second, at(11)).await.is_err());
    assert_eq!(
        store
            .finish_attempt(first, finish(AttemptResult::Rejected, AckLevel::None))
            .await
            .unwrap(),
        DeliveryState::Rejected
    );
    assert_eq!(store.begin_attempt(second, at(21)).await.unwrap(), 1);
}

#[tokio::test]
async fn close_requires_a_reason_and_rejects_an_in_flight_delivery() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    receipt.destinations = vec![destination()];
    let delivery = commit(&store, receipt, b"message").await.deliveries[0];
    for reason in ["  ".to_string(), "x".repeat(501)] {
        assert!(matches!(
            store
                .close_delivery(
                    CloseRequest {
                        delivery_id: delivery,
                        reason
                    },
                    at(5)
                )
                .await,
            Err(packie_store::Error::Rejected {
                code: RejectionCode::ReasonRequired,
                ..
            })
        ));
    }
    store.begin_attempt(delivery, at(10)).await.unwrap();
    assert!(matches!(
        store
            .close_delivery(
                CloseRequest {
                    delivery_id: delivery,
                    reason: "Investigated".into()
                },
                at(15)
            )
            .await,
        Err(packie_store::Error::Rejected {
            code: RejectionCode::DeliveryInProgress,
            ..
        })
    ));
    store
        .finish_attempt(delivery, finish(AttemptResult::Rejected, AckLevel::None))
        .await
        .unwrap();
    let closed = store
        .close_delivery(
            CloseRequest {
                delivery_id: delivery,
                reason: "  Superseded by corrected result  ".into(),
            },
            at(25),
        )
        .await
        .unwrap();
    assert_eq!(closed.state, DeliveryState::Cancelled);
    assert_eq!(
        closed.closed_reason.as_deref(),
        Some("Superseded by corrected result")
    );
    assert!(
        store
            .close_delivery(
                CloseRequest {
                    delivery_id: delivery,
                    reason: "Again".into()
                },
                at(26)
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn resend_checks_target_and_duplicate_risk_and_preserves_frozen_output() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    let policy = destination();
    receipt.destinations = vec![policy.clone()];
    let original = commit(&store, receipt, b"original").await;
    let delivery = original.deliveries[0];
    let queued = store
        .resend(
            resend_request(original.message_id, ResendMode::SameMessage),
            vec![policy.clone()],
            2,
            at(1),
        )
        .await
        .unwrap();
    assert_eq!(
        queued.destinations[0].rejected.as_ref().unwrap().code,
        RejectionCode::DeliveryInProgress
    );
    store
        .record_transformed_body(delivery, b"frozen output".to_vec(), 1)
        .await
        .unwrap();
    store.begin_attempt(delivery, at(10)).await.unwrap();
    store
        .finish_attempt(
            delivery,
            finish(AttemptResult::Acknowledged, AckLevel::Accepted),
        )
        .await
        .unwrap();
    let request = resend_request(original.message_id, ResendMode::SameMessage);
    let risky = store
        .resend(request.clone(), vec![policy.clone()], 2, at(30))
        .await
        .unwrap();
    assert_eq!(
        risky.destinations[0].rejected.as_ref().unwrap().code,
        RejectionCode::DuplicateRiskUnconfirmed
    );
    let mut missing = request.clone();
    missing.destinations = ResendTargets::Destinations(vec![ConnectorId::generate()]);
    let result = store
        .resend(missing, vec![policy.clone()], 2, at(30))
        .await
        .unwrap();
    assert_eq!(
        result.destinations[0].rejected.as_ref().unwrap().code,
        RejectionCode::DestinationNotFound
    );
    let mut confirmed = request;
    confirmed.confirm_duplicate_risk = true;
    let result = store
        .resend(confirmed, vec![policy], 2, at(30))
        .await
        .unwrap();
    assert_eq!(result.new_message_id, None);
    assert_eq!(result.destinations[0].delivery_id, Some(delivery));
    let record = store.delivery(delivery).unwrap();
    assert_eq!((record.attempts, record.total_attempts), (0, 1));
    assert_eq!(record.mapping_version, Some(1));
    assert_eq!(
        store.body(original.message_id, Some(delivery)).unwrap(),
        b"frozen output"
    );
    assert!(
        store
            .record_transformed_body(delivery, b"replacement".to_vec(), 2)
            .await
            .is_err()
    );
    assert_eq!(store.begin_attempt(delivery, at(30)).await.unwrap(), 2);
}

#[tokio::test]
async fn new_message_resend_links_original_and_reenters_pipeline_for_selected_targets() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    let first = destination();
    let second = destination();
    receipt.identifiers.source_identity = Some("source|identity".into());
    receipt.destinations = vec![first.clone(), second.clone()];
    let original = commit(&store, receipt, b"original").await;
    let mut request = resend_request(original.message_id, ResendMode::NewMessage);
    request.destinations = ResendTargets::Destinations(vec![second.id.parse().unwrap()]);
    let result = store
        .resend(request, vec![first, second.clone()], 3, at(30))
        .await
        .unwrap();
    let new = result.new_message_id.unwrap();
    let record = store.message(new).unwrap();
    assert_eq!(record.summary.origin, MessageOrigin::Resend);
    assert_eq!(record.summary.resent_from, Some(original.message_id));
    assert_eq!(record.summary.state, MessageState::Received);
    assert_eq!(record.summary.config_version, 3);
    assert!(record.duplicate_of.is_none());
    assert!(record.deliveries.is_empty());
    let targets = store.resend_targets(new).unwrap();
    assert_eq!(targets.as_slice(), std::slice::from_ref(&second.id));
    assert_eq!(store.body(new, None).unwrap(), b"original");
    let deliveries = store.route_received(new, vec![second], 3).await.unwrap();
    assert_eq!(deliveries.len(), 1);
    assert!(store.resend_targets(new).unwrap().is_empty());
    assert!(store.route_received(new, Vec::new(), 3).await.is_err());
    assert!(
        store
            .message(original.message_id)
            .unwrap()
            .deliveries
            .iter()
            .all(|delivery| delivery.state == DeliveryState::Queued)
    );
}

#[tokio::test]
async fn retention_keeps_unresolved_work_and_persistent_counters_after_history_is_purged() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    receipt.destinations = vec![destination()];
    let channel = receipt.channel_id.clone();
    let resolved = commit(&store, receipt.clone(), b"resolved").await;
    store
        .begin_attempt(resolved.deliveries[0], at(10))
        .await
        .unwrap();
    store
        .finish_attempt(
            resolved.deliveries[0],
            finish(AttemptResult::Acknowledged, AckLevel::Accepted),
        )
        .await
        .unwrap();
    let unresolved = commit(&store, receipt.clone(), b"unresolved").await;
    store
        .begin_attempt(unresolved.deliveries[0], at(10))
        .await
        .unwrap();
    store
        .finish_attempt(
            unresolved.deliveries[0],
            finish(AttemptResult::AckTimeout, AckLevel::HandedOff),
        )
        .await
        .unwrap();
    let queued = commit(&store, receipt.clone(), b"queued").await;
    receipt.destinations.clear();
    let pending = commit(&store, receipt, b"pending pipeline").await;
    let before = store.counters(&channel, None).unwrap();
    let policies = [(
        channel.clone(),
        Retention {
            max_age_days: Some(0),
            max_messages: Some(0),
        },
    )]
    .into_iter()
    .collect();
    let purged = store.purge(policies, at(86_400)).await.unwrap();
    assert_eq!(
        (
            purged.messages,
            purged.deliveries,
            purged.attempts,
            purged.bodies
        ),
        (1, 1, 1, 1)
    );
    assert!(matches!(
        store.message(resolved.message_id),
        Err(packie_store::Error::NotFound)
    ));
    for id in [unresolved.message_id, queued.message_id, pending.message_id] {
        assert!(store.message(id).is_ok());
    }
    assert_eq!(store.counters(&channel, None).unwrap(), before);
}

#[tokio::test]
async fn retention_never_purges_any_unresolved_delivery_state() {
    let (_directory, store) = opened();
    let receipt = receipt();
    let channel = receipt.channel_id.clone();
    let mut messages = Vec::new();
    for result in [
        AttemptResult::Rejected,
        AttemptResult::WriteFailed,
        AttemptResult::ConnectionLostAfterSend,
    ] {
        let mut receipt = receipt.clone();
        let mut policy = destination();
        policy.max_attempts = 1;
        receipt.destinations = vec![policy];
        let outcome = commit(&store, receipt, b"unresolved").await;
        store
            .begin_attempt(outcome.deliveries[0], at(10))
            .await
            .unwrap();
        store
            .finish_attempt(outcome.deliveries[0], finish(result, AckLevel::None))
            .await
            .unwrap();
        messages.push(outcome.message_id);
    }
    let policies = [(
        channel,
        Retention {
            max_age_days: Some(0),
            max_messages: Some(0),
        },
    )]
    .into_iter()
    .collect();
    assert_eq!(store.purge(policies, at(86_400)).await.unwrap().messages, 0);
    for message in messages {
        assert!(store.message(message).is_ok());
    }
}

#[tokio::test]
async fn retention_count_limit_removes_oldest_eligible_history_when_older_work_is_unresolved() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    let channel = receipt.channel_id.clone();
    let mut unresolved = Vec::new();
    let mut eligible = Vec::new();
    for second in 0..5 {
        receipt.received_at = at(second);
        let outcome = commit(&store, receipt.clone(), b"message").await;
        if second < 3 {
            unresolved.push(outcome.message_id);
        } else {
            store
                .record_filtered(outcome.message_id, "source_filter")
                .await
                .unwrap();
            eligible.push(outcome.message_id);
        }
    }
    let policies = [(
        channel,
        Retention {
            max_age_days: None,
            max_messages: Some(3),
        },
    )]
    .into_iter()
    .collect();
    assert_eq!(store.purge(policies, at(10)).await.unwrap().messages, 2);
    for id in unresolved {
        assert!(store.message(id).is_ok());
    }
    for id in eligible {
        assert!(matches!(
            store.message(id),
            Err(packie_store::Error::NotFound)
        ));
    }
}

#[tokio::test]
async fn destination_gauges_keep_historical_acknowledgements_and_errors_across_resend_cycles() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    let policy = destination();
    let channel = receipt.channel_id.clone();
    receipt.destinations = vec![policy.clone()];
    let original = commit(&store, receipt, b"message").await;
    let delivery = original.deliveries[0];
    store.begin_attempt(delivery, at(10)).await.unwrap();
    let mut failure = finish(AttemptResult::WriteFailed, AckLevel::None);
    failure.error_summary = Some("Connection closed before writing completed".into());
    store.finish_attempt(delivery, failure).await.unwrap();
    let queued = store
        .destination_counts(&channel, &policy.id, at(25))
        .unwrap();
    assert_eq!(queued.states.get("queued"), Some(&1));
    assert_eq!(queued.oldest_queued_at, Some(at(0)));
    assert_eq!(queued.oldest_queued_age_seconds, Some(25));
    assert_eq!(queued.next_attempt_at, Some(at(30)));
    assert_eq!(
        queued.last_error.as_deref(),
        Some("Connection closed before writing completed")
    );
    store.begin_attempt(delivery, at(30)).await.unwrap();
    let mut success = finish(AttemptResult::Acknowledged, AckLevel::Accepted);
    success.ended_at = at(40);
    store.finish_attempt(delivery, success).await.unwrap();
    let mut request = resend_request(original.message_id, ResendMode::SameMessage);
    request.confirm_duplicate_risk = true;
    store
        .resend(request, vec![policy.clone()], 2, at(50))
        .await
        .unwrap();
    let resent = store
        .destination_counts(&channel, &policy.id, at(50))
        .unwrap();
    assert_eq!(resent.last_ack_at, Some(at(40)));
    assert_eq!(resent.last_error, queued.last_error);
    assert_eq!((resent.totals.attempts, resent.totals.acknowledged), (2, 1));
    store.begin_attempt(delivery, at(50)).await.unwrap();
    let sending = store
        .destination_counts(&channel, &policy.id, at(51))
        .unwrap();
    assert_eq!(sending.states.get("sending"), Some(&1));
    assert_eq!(sending.oldest_queued_at, None);
    assert_eq!(sending.next_attempt_at, None);
    assert_eq!(sending.last_ack_at, Some(at(40)));
    assert_eq!(sending.last_error, queued.last_error);
}

#[tokio::test]
async fn startup_reconciliation_preserves_bytes_recovers_completed_receipts_and_closes_open_attempts()
 {
    let (directory, store) = opened();
    let mut receipt = receipt();
    let channel = receipt.channel_id.clone();
    receipt.initial_state = MessageState::Receiving;
    let interrupted = commit(&store, receipt.clone(), b"partial").await;
    let recovered = commit(&store, receipt.clone(), b"complete").await;
    receipt.initial_state = MessageState::Received;
    let pending = commit(&store, receipt.clone(), b"pending").await;
    receipt.destinations = vec![destination()];
    let sending = commit(&store, receipt, b"sent").await;
    store
        .begin_attempt(sending.deliveries[0], at(10))
        .await
        .unwrap();
    store.set_paused(&channel, true).await.unwrap();
    drop(store);
    let store = Store::open(directory.path(), OpenOptions::default()).unwrap();
    assert_eq!(
        store.receiving_messages().unwrap(),
        [interrupted.message_id, recovered.message_id]
    );
    let report = store
        .reconcile_with_recovered(vec![recovered.message_id])
        .await
        .unwrap();
    assert_eq!(
        (report.recovered, report.aborted, report.unknown),
        (1, 1, 1)
    );
    assert_eq!(report.requeued, [recovered.message_id, pending.message_id]);
    assert_eq!(report.paused_channels, [channel]);
    assert_eq!(
        store
            .message(interrupted.message_id)
            .unwrap()
            .abort_reason
            .as_deref(),
        Some("startup_interrupted")
    );
    assert_eq!(
        store.body(interrupted.message_id, None).unwrap(),
        b"partial"
    );
    let record = store.delivery(sending.deliveries[0]).unwrap();
    assert_eq!(record.state, DeliveryState::OutcomeUnknown);
    assert_eq!(record.reason_code.as_deref(), Some("engine_restarted"));
    assert_eq!(
        record.attempt_log[0].result,
        Some(AttemptResult::EngineStopped)
    );
    assert!(record.attempt_log[0].ended_at.is_some());
    let repeated = store.reconcile_at_startup().await.unwrap();
    assert_eq!(
        (repeated.recovered, repeated.aborted, repeated.unknown),
        (0, 0, 0)
    );
    assert_eq!(repeated.requeued, report.requeued);
}

#[tokio::test]
async fn repeated_alert_identity_updates_one_episode_and_a_new_raise_after_clear_starts_another() {
    let (_directory, store) = opened();
    let mut alert = Alert {
        id: packie_core::AlertId::generate(),
        identity: "channel:destination:attention".into(),
        code: "delivery_needs_attention".into(),
        severity: packie_core::Severity::Warning,
        channel_id: Some(ChannelId::generate().to_string()),
        endpoint_id: None,
        destination_id: None,
        summary: "Delivery requires attention".into(),
        next_action: "Inspect the delivery outcome".into(),
        first_seen_at: at(0),
        last_seen_at: at(0),
        cleared_at: None,
        occurrences: 1,
        details: serde_json::json!({"count":1}),
    };
    let first = store.record_alert_raised(alert.clone()).await.unwrap();
    assert_eq!(first, alert.id);
    alert.last_seen_at = at(10);
    alert.details = serde_json::json!({"count":2});
    assert_eq!(
        store.record_alert_raised(alert.clone()).await.unwrap(),
        first
    );
    let active = AlertFilter {
        active_only: true,
        ..AlertFilter::default()
    };
    let rows = store.alert_history(&active).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].first_seen_at,
            rows[0].last_seen_at,
            rows[0].occurrences
        ),
        (at(0), at(10), 2)
    );
    assert_eq!(rows[0].details, serde_json::json!({"count":2}));
    store
        .record_alert_cleared(&alert.identity, at(20))
        .await
        .unwrap();
    store
        .record_alert_cleared(&alert.identity, at(21))
        .await
        .unwrap();
    assert!(store.alert_history(&active).unwrap().is_empty());
    alert.first_seen_at = at(30);
    alert.last_seen_at = at(30);
    alert.id = packie_core::AlertId::generate();
    let next_episode_id = alert.id;
    let second = store.record_alert_raised(alert).await.unwrap();
    assert_eq!(second, next_episode_id);
    assert_ne!(first, second);
    let rows = store.alert_history(&AlertFilter::default()).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].id, second);
    assert_eq!(rows[1].cleared_at, Some(at(20)));
}

fn seed_grid(store: &Store, count: u32) -> ChannelId {
    let mut conn = Connection::open(store.database_path()).unwrap();
    let channel = ChannelId::generate();
    let endpoint = EndpointId::generate();
    let tx = conn.transaction().unwrap();
    {
        let mut insert = tx.prepare("INSERT INTO messages(id,channel_id,channel_label,seq,received_at,direction,origin,protocol,state,byte_length,content_hash,source_endpoint_id,config_version,control_id,searchable_text) VALUES(?1,?2,'Results',?3,?4,'inbound','received','hl7_mllp',?5,?3,'synthetic',?6,1,?7,?8)").unwrap();
        for seq in 1..=count {
            insert
                .execute(params![
                    MessageId::generate(),
                    channel.to_string(),
                    seq,
                    at(i64::from(seq)),
                    if seq % 2 == 0 { "invalid" } else { "received" },
                    endpoint.to_string(),
                    format!("control-{seq:06}"),
                    if seq % 10 == 0 {
                        "searchable marker"
                    } else {
                        "other metadata"
                    }
                ])
                .unwrap();
        }
    }
    tx.commit().unwrap();
    channel
}

#[tokio::test]
async fn grid_filters_sort_and_page_a_few_thousand_rows_with_consistent_totals() {
    let (_directory, store) = opened();
    let channel = seed_grid(&store, 3000);
    let mut query = MessageQuery {
        sort: MessageSort {
            field: MessageSortField::ByteLength,
            descending: false,
        },
        offset: 20,
        limit: 7,
        ..MessageQuery::default()
    };
    query.filter.channels = vec![channel];
    query.filter.states = vec![MessageState::Invalid];
    query.filter.text = Some("searchable marker".into());
    query.filter.since = Some(at(100));
    query.filter.until = Some(at(1000));
    let page = store.query_messages(&query).unwrap();
    assert_eq!(page.total, 90);
    assert_eq!((page.offset, page.limit), (20, 7));
    assert_eq!(
        page.messages.iter().map(|row| row.seq).collect::<Vec<_>>(),
        (300..=360).step_by(10).collect::<Vec<_>>()
    );
    query.filter.identifier = Some(IdentifierFilter {
        value: "control-0003".into(),
        prefix: true,
    });
    assert_eq!(store.query_messages(&query).unwrap().total, 10);
    query.offset = 0;
    query.sort.descending = true;
    assert_eq!(store.query_messages(&query).unwrap().messages[0].seq, 390);
    query.filter.identifier = Some(IdentifierFilter {
        value: "control-000300".into(),
        prefix: false,
    });
    assert_eq!(store.query_messages(&query).unwrap().total, 1);
}

#[tokio::test]
#[ignore = "200,000-row Messages-grid timing check; run explicitly on an otherwise idle machine"]
async fn indexed_grid_page_over_two_hundred_thousand_rows_stays_under_250ms() {
    let (_directory, store) = opened();
    let channel = seed_grid(&store, 200_000);
    let mut query = MessageQuery::default();
    query.filter.channels = vec![channel];
    query.filter.states = vec![MessageState::Invalid];
    query.filter.since = Some(at(100_000));
    query.limit = 100;
    let started = Instant::now();
    let page = store.query_messages(&query).unwrap();
    let elapsed = started.elapsed();
    assert_eq!(page.total, 50_001);
    assert_eq!(page.messages.len(), 100);
    assert_eq!(page.messages[0].seq, 200_000);
    assert!(
        elapsed < Duration::from_millis(250),
        "grid query took {elapsed:?}"
    );
}

/// D-11 / PRF-03: one Messages-grid page per filter kind over 500,000 rows, each timed.
#[tokio::test]
#[ignore = "500,000-row Messages-grid timing check (acceptance D-11); run explicitly on an otherwise idle machine"]
async fn grid_pages_over_five_hundred_thousand_rows_by_every_filter_stay_under_250ms() {
    let (_directory, store) = opened();
    let channel = seed_grid(&store, 500_000);
    let base = || MessageQuery {
        limit: 100,
        ..MessageQuery::default()
    };
    let mut cases: Vec<(&str, MessageQuery, u64)> = Vec::new();
    let mut by_time = base();
    by_time.filter.since = Some(at(400_001));
    cases.push(("time", by_time, 100_000));
    let mut by_channel = base();
    by_channel.filter.channels = vec![channel];
    cases.push(("channel", by_channel, 500_000));
    let mut by_state = base();
    by_state.filter.states = vec![MessageState::Invalid];
    cases.push(("state", by_state, 250_000));
    let mut by_identifier = base();
    by_identifier.filter.identifier = Some(IdentifierFilter {
        value: "control-4321".into(),
        prefix: true,
    });
    cases.push(("identifier prefix", by_identifier, 100));
    let mut by_text = base();
    by_text.filter.text = Some("searchable marker".into());
    cases.push(("text", by_text, 50_000));
    let mut deep = base();
    deep.filter.channels = vec![channel];
    deep.offset = 499_900;
    cases.push(("channel, offset 499,900", deep, 500_000));
    let mut timings = String::new();
    let mut slowest = Duration::ZERO;
    for (name, query, total) in cases {
        let started = Instant::now();
        let page = store.query_messages(&query).unwrap();
        let elapsed = started.elapsed();
        writeln!(timings, "{name}: {elapsed:?}, total {}", page.total).unwrap();
        assert_eq!(page.total, total, "{name}");
        assert_eq!(page.messages.len(), 100, "{name}");
        slowest = slowest.max(elapsed);
    }
    // Where the acceptance record wants the numbers.
    if let Ok(path) = std::env::var("PACKIE_GRID_TIMINGS") {
        std::fs::write(path, &timings).unwrap();
    }
    assert!(
        slowest < Duration::from_millis(250),
        "a grid query took {slowest:?}:\n{timings}"
    );
}

#[tokio::test]
async fn host_routing_queue_head_abandon_and_totals() {
    let (_dir, store) = opened();
    let committed = commit(&store, receipt(), b"MSH|orig").await;
    let lis = destination();
    let failing = Destination {
        transform_error: Some("rule 2 (lookup) at OBX.8".into()),
        ..destination()
    };
    let ids = store
        .route_with_outputs(
            committed.message_id,
            vec![(lis.clone(), Some(b"MSH|mapped".to_vec())), (failing, None)],
            3,
        )
        .await
        .unwrap();
    let message = store.message(committed.message_id).unwrap();
    assert_eq!(message.summary.state, MessageState::Routed);
    assert_eq!(message.deliveries[1].state, DeliveryState::Failed);

    // The head of the queue carries the output and the original in one read.
    let head = store
        .next_delivery(&receipt_channel(&store, committed.message_id), &lis.id)
        .unwrap()
        .unwrap();
    assert_eq!(head.delivery_id, ids[0]);
    assert_eq!(head.body.as_deref(), Some(&b"MSH|mapped"[..]));
    assert_eq!(head.original, b"MSH|orig");

    // A stop before the last byte: the attempt does not count and the delivery waits.
    store.begin_attempt(ids[0], at(10)).await.unwrap();
    assert!(
        store
            .next_delivery(&head_channel(&store, ids[0]), &lis.id)
            .unwrap()
            .is_none(),
        "one in flight"
    );
    let state = store
        .abandon_attempt(ids[0], "channel_paused".into(), at(11))
        .await
        .unwrap();
    assert_eq!(state, DeliveryState::Queued);
    let record = store.delivery(ids[0]).unwrap();
    assert_eq!((record.attempts, record.total_attempts), (0, 0));
    assert_eq!(record.reason_code.as_deref(), Some("channel_paused"));
    assert!(record.attempt_log.is_empty());

    let totals = store.totals().unwrap();
    assert_eq!(
        (
            totals.messages,
            totals.deliveries_queued,
            totals.deliveries_unresolved
        ),
        (1, 1, 1)
    );
    assert!(totals.size_bytes > 0);
}

#[tokio::test]
async fn completing_a_transfer_replaces_the_frames_with_the_assembled_original() {
    let (_dir, store) = opened();
    let mut receiving = receipt();
    receiving.initial_state = MessageState::Receiving;
    receiving.protocol = ProtocolKind::Astm;
    let started = commit(&store, receiving.clone(), b"\x021H|...\x03\r\n").await;
    store
        .append_receiving(
            started.message_id,
            b"\x022L|1\x03\r\n".to_vec(),
            false,
            Identifiers::default(),
            true,
        )
        .await
        .unwrap();
    let ids = Identifiers {
        source_identity: Some("SIM|20260101|S1".into()),
        ..Identifiers::default()
    };
    let completed = store
        .complete_receiving(
            started.message_id,
            b"H|...\rL|1\r".to_vec(),
            ids.clone(),
            true,
        )
        .await
        .unwrap();
    assert_eq!(completed.duplicate_of, None);
    assert_eq!(
        store.body(started.message_id, None).unwrap(),
        b"H|...\rL|1\r"
    );
    assert_eq!(
        store.message(started.message_id).unwrap().summary.state,
        MessageState::Received
    );

    // The same records again from the same sender: a duplicate.
    let again = commit(&store, receiving, b"\x021H|...\x03\r\n").await;
    let duplicate = store
        .complete_receiving(again.message_id, b"H|...\rL|1\r".to_vec(), ids, true)
        .await
        .unwrap();
    assert_eq!(duplicate.duplicate_of, Some(started.message_id));
}

#[test]
fn delivery_rules_are_one_public_function() {
    let policy = packie_store::FrozenPolicy {
        max_attempts: 2,
        retry_unacknowledged: false,
        ack_required: AckLevel::Accepted,
    };
    let rule = |result, level, attempts| {
        packie_store::delivery_state_after(&finish(result, level), attempts, &policy)
    };
    assert_eq!(
        rule(AttemptResult::WriteFailed, AckLevel::None, 1).unwrap(),
        DeliveryState::Queued
    );
    assert_eq!(
        rule(AttemptResult::WriteFailed, AckLevel::None, 2).unwrap(),
        DeliveryState::Failed
    );
    assert_eq!(
        rule(AttemptResult::AckTimeout, AckLevel::Received, 1).unwrap(),
        DeliveryState::TimedOut
    );
    assert_eq!(
        rule(AttemptResult::EngineStopped, AckLevel::None, 1).unwrap(),
        DeliveryState::OutcomeUnknown
    );
    assert!(rule(AttemptResult::Acknowledged, AckLevel::Received, 1).is_err());
}

fn receipt_channel(store: &Store, message: MessageId) -> String {
    store
        .message(message)
        .unwrap()
        .summary
        .channel_id
        .to_string()
}

fn head_channel(store: &Store, delivery: packie_core::DeliveryId) -> String {
    store.delivery(delivery).unwrap().channel_id.to_string()
}

#[tokio::test]
async fn a_batch_commits_every_unit_in_order_with_duplicate_checks_inside_it() {
    let (_directory, store) = opened();
    let row = |rows: &str| Receipt {
        identifiers: Identifiers {
            source_identity: Some(format!("batch.csv|{rows}")),
            ..Identifiers::default()
        },
        source_file: Some(packie_ipc::messages::SourceFile {
            name: "batch.csv".into(),
            file_hash: packie_store::content_hash(b"whole file"),
            first_row: 1,
            last_row: 1,
        }),
        protocol: ProtocolKind::Delimited,
        ..receipt()
    };
    let first = row("1-1");
    let channel = first.channel_id.clone();
    let second = Receipt {
        channel_id: channel.clone(),
        ..row("2-2")
    };
    let repeated = Receipt {
        channel_id: channel.clone(),
        ..row("1-1")
    };
    let outcomes = store
        .commit_received_batch(vec![
            (first, b"h\r\nA\r\n".to_vec()),
            (second, b"h\r\nB\r\n".to_vec()),
            (repeated, b"h\r\nA\r\n".to_vec()),
        ])
        .await
        .unwrap();
    assert_eq!(outcomes.len(), 3);
    assert_eq!(outcomes[1].sequence, outcomes[0].sequence + 1);
    assert_eq!(outcomes[2].duplicate_of, Some(outcomes[0].message_id));
    let record = store.message(outcomes[1].message_id).unwrap();
    assert_eq!(record.summary.state, MessageState::Received);
    assert_eq!(record.source_file.unwrap().name, "batch.csv");
    assert_eq!(
        store.body(outcomes[1].message_id, None).unwrap(),
        b"h\r\nB\r\n"
    );
    let counts = store.counters(&channel, None).unwrap();
    assert_eq!((counts.received, counts.duplicates), (3, 1));
}

#[tokio::test]
async fn the_queue_head_follows_receipt_order_when_deliveries_are_created_out_of_order() {
    let (_directory, store) = opened();
    let first_receipt = receipt();
    let channel = first_receipt.channel_id.clone();
    let lis = destination();
    let first = commit(&store, first_receipt.clone(), b"first").await;
    let second = commit(&store, first_receipt, b"second").await;
    // Startup reconciliation may route a later message first.
    store
        .route_received(second.message_id, vec![lis.clone()], 1)
        .await
        .unwrap();
    store
        .route_received(first.message_id, vec![lis.clone()], 1)
        .await
        .unwrap();
    let head = store.next_delivery(&channel, &lis.id).unwrap().unwrap();
    assert_eq!(head.message_id, first.message_id);
    assert_eq!(head.sequence, first.sequence);
}

/// A destination with a large backlog: reading the head stays one index lookup.
#[tokio::test]
#[ignore = "20,000 queued deliveries timing check; run explicitly on an otherwise idle machine"]
async fn the_queue_head_of_twenty_thousand_queued_deliveries_reads_in_under_5ms() {
    let (_directory, store) = opened();
    let lis = destination();
    let mut base = receipt();
    base.destinations = vec![lis.clone()];
    let channel = base.channel_id.clone();
    let units: Vec<(Receipt, Vec<u8>)> = (0..20_000)
        .map(|n| (base.clone(), format!("message {n}").into_bytes()))
        .collect();
    store.commit_received_batch(units).await.unwrap();
    let started = Instant::now();
    for _ in 0..100 {
        assert!(store.next_delivery(&channel, &lis.id).unwrap().is_some());
    }
    let each = started.elapsed() / 100;
    assert!(each < Duration::from_millis(5), "queue head took {each:?}");
}

/// A channel with a long history: committing a new message (duplicate and revision
/// checks, original body) does not read the history.
#[tokio::test]
#[ignore = "20,000-message commit timing check; run explicitly on an otherwise idle machine"]
async fn committing_into_a_twenty_thousand_message_channel_stays_under_5ms() {
    let (_directory, store) = opened();
    let base = receipt();
    let with_identity = |n: usize| Receipt {
        identifiers: Identifiers {
            source_identity: Some(format!("SIM|FAC|C-{n}")),
            ..Identifiers::default()
        },
        ..base.clone()
    };
    let history: Vec<(Receipt, Vec<u8>)> = (0..20_000)
        .map(|n| (with_identity(n), format!("message {n}").into_bytes()))
        .collect();
    store.commit_received_batch(history).await.unwrap();
    let started = Instant::now();
    for n in 20_000..20_100 {
        commit(
            &store,
            with_identity(n),
            format!("message {n}").into_bytes(),
        )
        .await;
    }
    let each = started.elapsed() / 100;
    assert!(each < Duration::from_millis(5), "a commit took {each:?}");
}

#[tokio::test]
async fn a_resend_reason_is_kept_with_the_attempt_cycle_it_started() {
    let (_directory, store) = opened();
    let lis = destination();
    let mut first = receipt();
    first.destinations = vec![lis.clone()];
    let committed = commit(&store, first, b"message").await;
    let delivery = committed.deliveries[0];
    store.begin_attempt(delivery, at(1)).await.unwrap();
    store
        .finish_attempt(delivery, finish(AttemptResult::Rejected, AckLevel::None))
        .await
        .unwrap();

    let mut same = resend_request(committed.message_id, ResendMode::SameMessage);
    same.reason = Some("  partner fixed the code table  ".into());
    store
        .resend(same, vec![lis.clone()], 1, at(10))
        .await
        .unwrap();
    assert_eq!(
        store.delivery(delivery).unwrap().resend_reason.as_deref(),
        Some("partner fixed the code table")
    );
    // A later resend without a reason starts a cycle without one.
    store
        .close_delivery(
            CloseRequest {
                delivery_id: delivery,
                reason: "checked with the partner".into(),
            },
            at(15),
        )
        .await
        .unwrap();
    store
        .resend(
            resend_request(committed.message_id, ResendMode::SameMessage),
            vec![lis.clone()],
            1,
            at(20),
        )
        .await
        .unwrap();
    assert_eq!(store.delivery(delivery).unwrap().resend_reason, None);

    let mut new = resend_request(committed.message_id, ResendMode::NewMessage);
    new.reason = Some("re-run with mapping v2".into());
    new.confirm_duplicate_risk = true;
    let created = store
        .resend(new, vec![lis.clone()], 2, at(30))
        .await
        .unwrap()
        .new_message_id
        .unwrap();
    let ids = store.route_received(created, vec![lis], 2).await.unwrap();
    assert_eq!(
        store.delivery(ids[0]).unwrap().resend_reason.as_deref(),
        Some("re-run with mapping v2")
    );
}

#[tokio::test]
async fn messages_filter_by_an_id_prefix_in_any_case_with_or_without_dashes() {
    let (_directory, store) = opened();
    let a = commit(&store, receipt(), b"a").await.message_id;
    let _b = commit(&store, receipt(), b"b").await.message_id;
    let full = a.to_string();
    // The first 12 hex digits of a UUIDv7 are its millisecond, which messages committed in
    // the same millisecond share; 24 digits reach the random part and name one message.
    let digits = full.replace('-', "");
    let typed = digits.get(..24).unwrap().to_uppercase();
    let mut query = MessageQuery::default();
    query.filter.message_id_prefix = Some(typed);
    let page = store.query_messages(&query).unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.messages[0].message_id, a);
    query.filter.message_id_prefix = Some(digits.get(..12).unwrap().to_owned());
    let by_time = store.query_messages(&query).unwrap();
    assert!(by_time.messages.iter().any(|m| m.message_id == a));
    query.filter.message_id_prefix = Some(full);
    assert_eq!(store.query_messages(&query).unwrap().total, 1);
}

#[tokio::test]
async fn retention_keeps_an_original_while_its_revision_is_kept() {
    let (_directory, store) = opened();
    let mut receipt = receipt();
    receipt.identifiers.source_identity = Some("sender|facility|control".into());
    receipt.destinations = vec![destination()];
    let channel = receipt.channel_id.clone();
    let acknowledge = |delivery| {
        let store = &store;
        async move {
            store.begin_attempt(delivery, at(10)).await.unwrap();
            store
                .finish_attempt(
                    delivery,
                    finish(AttemptResult::Acknowledged, AckLevel::Accepted),
                )
                .await
                .unwrap();
        }
    };
    let original = commit(&store, receipt.clone(), b"first").await;
    acknowledge(original.deliveries[0]).await;
    let revision = commit(&store, receipt, b"corrected").await;
    assert_eq!(revision.revision_of, Some(original.message_id));
    let everything = || {
        [(
            channel.clone(),
            Retention {
                max_age_days: Some(0),
                max_messages: Some(0),
            },
        )]
        .into_iter()
        .collect()
    };

    // The revision is still being delivered, so neither goes.
    assert_eq!(
        store
            .purge(everything(), at(86_400))
            .await
            .unwrap()
            .messages,
        0
    );
    assert!(store.message(original.message_id).is_ok());

    // Once the revision may go, it goes first; the original follows on the next run.
    acknowledge(revision.deliveries[0]).await;
    assert_eq!(
        store
            .purge(everything(), at(86_400))
            .await
            .unwrap()
            .messages,
        1
    );
    assert!(store.message(revision.message_id).is_err());
    assert!(store.message(original.message_id).is_ok());
    assert_eq!(
        store
            .purge(everything(), at(86_400))
            .await
            .unwrap()
            .messages,
        1
    );
    assert!(store.message(original.message_id).is_err());
}
