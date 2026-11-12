use packie_core::{AckLevel, AttemptResult, ConnectorId, DeliveryState, Timestamp};

use crate::Destination;
use crate::testing::{completion, fixture};

#[tokio::test]
async fn queue_order_and_frozen_attempt_limit_are_enforced() {
    let (_directory, store, mut receipt, _) = fixture();
    receipt.destinations[0].max_attempts = 2;
    let first = store
        .commit_received(receipt.clone(), vec![1])
        .await
        .unwrap()
        .deliveries[0];
    let second = store
        .commit_received(receipt, vec![2])
        .await
        .unwrap()
        .deliveries[0];
    assert!(store.begin_attempt(second, Timestamp::now()).await.is_err());
    assert_eq!(
        store.begin_attempt(first, Timestamp::now()).await.unwrap(),
        1
    );
    assert!(store.begin_attempt(second, Timestamp::now()).await.is_err());
    let mut finish = completion(AttemptResult::WriteFailed, AckLevel::None);
    finish.retry_at = Some(
        finish
            .ended_at
            .checked_add(std::time::Duration::from_secs(10))
            .unwrap(),
    );
    assert_eq!(
        store.finish_attempt(first, finish.clone()).await.unwrap(),
        DeliveryState::Queued
    );
    assert!(store.begin_attempt(first, finish.ended_at).await.is_err());
    let due = finish.retry_at.unwrap();
    assert_eq!(store.begin_attempt(first, due).await.unwrap(), 2);
    finish.ended_at = due;
    finish.retry_at = None;
    assert_eq!(
        store.finish_attempt(first, finish).await.unwrap(),
        DeliveryState::Failed
    );
    assert!(store.begin_attempt(first, due).await.is_err());
    assert_eq!(store.begin_attempt(second, due).await.unwrap(), 1);
}

#[tokio::test]
async fn required_ack_cannot_finish_as_failure() {
    let (_directory, store, receipt, _) = fixture();
    let id = store
        .commit_received(receipt, vec![1])
        .await
        .unwrap()
        .deliveries[0];
    store.begin_attempt(id, Timestamp::now()).await.unwrap();
    for result in AttemptResult::ALL
        .iter()
        .copied()
        .filter(|result| *result != AttemptResult::Acknowledged)
    {
        assert!(
            store
                .finish_attempt(id, completion(result, AckLevel::Accepted))
                .await
                .is_err()
        );
        let record = store.delivery(id).unwrap();
        assert_eq!(record.state, DeliveryState::Sending);
        assert!(record.attempt_log[0].ended_at.is_none());
    }
    assert_eq!(
        store
            .finish_attempt(
                id,
                completion(AttemptResult::Acknowledged, AckLevel::Accepted)
            )
            .await
            .unwrap(),
        DeliveryState::Acknowledged
    );
}

#[tokio::test]
async fn lower_ack_remains_sending_and_is_retained_on_timeout() {
    let (_directory, store, receipt, _) = fixture();
    let id = store
        .commit_received(receipt, vec![1, 2])
        .await
        .unwrap()
        .deliveries[0];
    store.begin_attempt(id, Timestamp::now()).await.unwrap();
    store
        .record_ack_progress(id, AckLevel::Received, Some("CA".into()), Timestamp::now())
        .await
        .unwrap();
    assert_eq!(store.delivery(id).unwrap().state, DeliveryState::Sending);
    assert!(
        store
            .finish_attempt(
                id,
                completion(AttemptResult::Acknowledged, AckLevel::Received)
            )
            .await
            .is_err()
    );
    assert!(
        store
            .record_ack_progress(id, AckLevel::HandedOff, None, Timestamp::now())
            .await
            .is_err()
    );
    assert_eq!(
        store
            .finish_attempt(
                id,
                completion(AttemptResult::AckTimeout, AckLevel::Received)
            )
            .await
            .unwrap(),
        DeliveryState::TimedOut
    );
    let record = store.delivery(id).unwrap();
    assert_eq!(record.ack_level, AckLevel::Received);
    assert_eq!(record.attempt_log[0].ack_code.as_deref(), Some("CA"));
}

#[tokio::test]
async fn unacknowledged_retry_is_explicit_and_bounded() {
    let (_directory, store, mut receipt, _) = fixture();
    receipt.destinations[0].retry_unacknowledged = true;
    receipt.destinations[0].max_attempts = 2;
    let id = store
        .commit_received(receipt, vec![1])
        .await
        .unwrap()
        .deliveries[0];
    store.begin_attempt(id, Timestamp::now()).await.unwrap();
    let mut finish = completion(AttemptResult::AckTimeout, AckLevel::HandedOff);
    assert!(store.finish_attempt(id, finish.clone()).await.is_err());
    finish.retry_at = Some(finish.ended_at);
    assert_eq!(
        store.finish_attempt(id, finish).await.unwrap(),
        DeliveryState::Queued
    );
    store.begin_attempt(id, Timestamp::now()).await.unwrap();
    assert_eq!(
        store
            .finish_attempt(
                id,
                completion(AttemptResult::AckTimeout, AckLevel::HandedOff)
            )
            .await
            .unwrap(),
        DeliveryState::TimedOut
    );
    let counters = store
        .read(|conn| {
            Ok(conn.query_row(
        "SELECT attempts,errors,timed_out,bytes_out FROM channel_counters WHERE destination_id=''",
        [],|r|Ok((r.get::<_,u64>(0)?,r.get::<_,u64>(1)?,r.get::<_,u64>(2)?,r.get::<_,u64>(3)?)))?)
        })
        .unwrap();
    assert_eq!(counters, (2, 2, 2, 2));
}

#[tokio::test]
async fn removal_cancels_unresolved_work_and_preserves_sending_and_success() {
    let (_directory, store, mut receipt, _) = fixture();
    receipt
        .destinations
        .push(Destination::new(ConnectorId::generate(), "Other"));
    let outcome = store
        .commit_received(receipt.clone(), vec![1])
        .await
        .unwrap();
    let first = outcome.deliveries[0];
    let second = outcome.deliveries[1];
    store.begin_attempt(first, Timestamp::now()).await.unwrap();
    store
        .record_transform_error(second, "cannot map".into(), 1, Timestamp::now())
        .await
        .unwrap();
    assert_eq!(
        store
            .cancel_removed(
                &receipt.channel_id,
                Some(receipt.destinations[1].id.clone()),
                Timestamp::now()
            )
            .await
            .unwrap(),
        1
    );
    assert_eq!(store.delivery(first).unwrap().state, DeliveryState::Sending);
    let record = store.delivery(second).unwrap();
    assert_eq!(record.state, DeliveryState::Cancelled);
    assert_eq!(record.reason_code.as_deref(), Some("destination_removed"));
    assert!(record.closed_reason.is_none());
    store
        .finish_attempt(
            first,
            completion(AttemptResult::Acknowledged, AckLevel::Accepted),
        )
        .await
        .unwrap();
    receipt.destinations.pop();
    let queued = store
        .commit_received(receipt.clone(), vec![2])
        .await
        .unwrap()
        .deliveries[0];
    assert_eq!(
        store
            .cancel_removed(&receipt.channel_id, None, Timestamp::now())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        store.delivery(queued).unwrap().reason_code.as_deref(),
        Some("channel_removed")
    );
    assert_eq!(
        store.delivery(first).unwrap().state,
        DeliveryState::Acknowledged
    );
    assert_eq!(
        store.delivery(second).unwrap().reason_code.as_deref(),
        Some("destination_removed")
    );
    assert_eq!(
        store
            .cancel_removed(&receipt.channel_id, None, Timestamp::now())
            .await
            .unwrap(),
        0
    );
}
