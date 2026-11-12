/**
 * The event names of the engine's log catalog (diagnostics.md 7.3), offered as suggestions in the
 * Event filter. The engine matches the whole name, so a name outside the list can still be typed.
 */
export const LOG_EVENT_NAMES: readonly string[] = [
  "node.started",
  "node.stopped",
  "config.applied",
  "config.rejected",
  "channel.started",
  "channel.stopped",
  "channel.state_changed",
  "bridge.session_opened",
  "bridge.session_closed",
  "bridge.close_propagated",
  "endpoint.connected",
  "endpoint.retry_scheduled",
  "endpoint.retry_succeeded",
  "endpoint.write_failed",
  "endpoint.state_changed",
  "listener.peer_rejected",
  "buffer.overflow_detected",
  "buffer.overflow_cleared",
  "tap.dropped",
  "message.committed",
  "message.duplicate",
  "message.identity_reused",
  "message.invalid",
  "message.aborted",
  "message.filtered",
  "delivery.attempt_started",
  "delivery.state_changed",
  "delivery.transform_failed",
  "delivery.ack_unmatched",
  "delivery.resend_requested",
  "retention.purged",
  "reconciliation.completed",
  "store.error",
  "alert.raised",
  "alert.cleared",
  "operator.command",
];

export const isKnownLogEvent = (name: string): boolean => LOG_EVENT_NAMES.includes(name);
