// What the mock console's actions do to the audit log (docs/spec/audit.md section 3): commands
// are recorded `requested` then with their outcome, content views before the content is
// returned, and none of them runs when the log cannot be written.
import type {
  BodyRequest,
  Delivery,
  MessageBody,
  MessageExportRequest,
  MessageFields,
  MessagePage,
  MessageQuery,
  ReportFile,
  ResendRequest,
  ResendResult,
} from "../types";
import { exportMessagesText } from "./messageExport";
import type { CommandKind } from "./runtimeFactory";
import type { MockWorld } from "./world";

const COMMAND_ACTIONS = {
  pause: "channel_pause",
  resume: "channel_resume",
  restart: "channel_restart",
} as const;

export function channelCommand(world: MockWorld, kind: CommandKind, id: string): void {
  world.audit.command({ action: COMMAND_ACTIONS[kind], objectType: "channel", objectId: id }, () =>
    world.command(kind, id),
  );
}

export function resend(world: MockWorld, request: ResendRequest): ResendResult {
  return world.audit.command(
    {
      action: "message_resend",
      objectType: "message",
      objectId: request.messageId,
      details: {
        destinations: request.destinations,
        mode: request.mode,
        confirm_duplicate_risk: request.confirmDuplicateRisk,
        confirm_superseded: request.confirmSuperseded ?? false,
        reason: request.reason ?? null,
      },
    },
    () => world.messages.resend(request),
  );
}

export function closeDelivery(world: MockWorld, deliveryId: string, reason: string): Delivery {
  return world.audit.command(
    { action: "delivery_close", objectType: "delivery", objectId: deliveryId, details: { reason } },
    () => world.messages.closeDelivery(deliveryId, reason),
  );
}

export function secret(
  world: MockWorld,
  action: "secret_set" | "secret_delete",
  name: string,
  run: () => void,
): void {
  world.audit.command({ action, objectType: "secret", objectId: name }, run);
}

/** A search by identifier, text or id prefix is recorded; browsing by channel, state or time is not. */
export function listMessages(world: MockWorld, query: MessageQuery): Promise<MessagePage> {
  const { identifier, text, messageIdPrefix, channelIds, since, until } = query.filter;
  if (identifier !== undefined || text !== undefined || messageIdPrefix !== undefined) {
    world.audit.record({
      action: "message_search",
      details: {
        identifier: identifier && !identifier.prefix ? identifier.value : null,
        identifier_prefix: identifier?.prefix ? identifier.value : null,
        text: text ?? null,
        message_id_prefix: messageIdPrefix ?? null,
        channels: channelIds ?? null,
        since: since ?? null,
        until: until ?? null,
      },
    });
  }
  return world.messages.listInSteps(query);
}

/**
 * An export is recorded once, with the filter, the format, whether content is included and
 * how many messages match; nothing is written while the log cannot be written.
 */
export function exportMessages(
  world: MockWorld,
  request: MessageExportRequest,
): { content: string; rows: number } {
  const { content, rows, matching } = exportMessagesText(world, request);
  world.audit.record({
    action: "message_export",
    details: {
      filter: request.filter,
      format: request.format,
      include_bodies: request.includeBodies,
      count: matching,
    },
  });
  return { content, rows };
}

/**
 * An export the console builds itself is recorded before it is written; with the audit log
 * unwritable this throws and the file is not written.
 */
export function recordClientExport(world: MockWorld, file: ReportFile): void {
  if (file.audit === undefined) return;
  world.audit.command(
    {
      action: "client_export",
      details: { kind: file.audit.kind, rows: file.audit.rows },
    },
    () => undefined,
  );
}

export function messageBody(world: MockWorld, request: BodyRequest): MessageBody {
  world.audit.record({
    action: "message_body_view",
    objectType: "message",
    objectId: request.messageId,
    details: {
      body: request.which.kind,
      delivery_id: request.which.kind === "transformed" ? request.which.deliveryId : null,
    },
  });
  return world.messages.body(request);
}

export function messageFields(world: MockWorld, id: string): MessageFields {
  world.audit.record({ action: "message_fields_view", objectType: "message", objectId: id });
  return world.messages.fields(id);
}
