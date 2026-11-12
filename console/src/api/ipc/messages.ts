/**
 * Messages against the engine's store (`get_messages`, `get_message`, `get_message_fields`,
 * `get_message_body`,
 * `resend`, `close_delivery`). An engine without a store answers `not_implemented`, which
 * reaches the pages as an `EngineError` with that code. Nothing here reads a body unless a
 * page asks for one.
 */

import type {
  BodyRequest,
  Delivery,
  MessageBody,
  MessageDetail,
  MessageExportRequest,
  MessageExportResult,
  MessageFields,
  MessagePage,
  MessageQuery,
  ResendRequest,
  ResendResult,
  TransmissionQuery,
  TransmissionReport,
} from "../types";
import type { TauriBridge } from "./bridge";
import type { IpcClient } from "./client";
import type { ConfigCache } from "./configCache";
import { fromCommandError } from "./errors";
import {
  bodyFromIpc,
  bodyRequestToIpc,
  deliveryFromIpc,
  detailFromIpc,
  fieldsFromIpc,
  queryToIpc,
  resendFromIpc,
  resendToIpc,
  summaryFromIpc,
} from "./messageMap";
import { exportToIpc, transmissionFromIpc, transmissionToIpc } from "./reports";

export class MessageApi {
  constructor(
    private readonly client: IpcClient,
    private readonly configs: ConfigCache,
    private readonly bridge: TauriBridge,
  ) {}

  async list(query: MessageQuery): Promise<MessagePage> {
    const page = await this.client.request(
      { type: "get_messages", ...queryToIpc(query) },
      "messages",
    );
    return {
      rows: page.messages.map(summaryFromIpc),
      total: page.total,
      offset: page.offset,
      limit: page.limit,
    };
  }

  async get(id: string): Promise<MessageDetail> {
    const { message } = await this.client.request(
      { type: "get_message", message_id: id },
      "message",
    );
    // The label is a convenience: a document that cannot be read leaves the endpoint's id.
    const config = await this.configs.current().catch(() => null);
    const endpoint = config?.endpoints?.find((e) => e.endpoint_id === message.source_endpoint_id);
    return detailFromIpc(message, endpoint?.label);
  }

  async fields(id: string): Promise<MessageFields> {
    const fields = await this.client.request(
      { type: "get_message_fields", message_id: id },
      "message_fields",
    );
    return fieldsFromIpc(fields);
  }

  async body(request: BodyRequest): Promise<MessageBody> {
    const body = await this.client.request(
      { type: "get_message_body", ...bodyRequestToIpc(request) },
      "message_body",
    );
    return bodyFromIpc(body);
  }

  async resend(request: ResendRequest): Promise<ResendResult> {
    const result = await this.client.request({ type: "resend", ...resendToIpc(request) }, "resent");
    return resendFromIpc(result);
  }

  async closeDelivery(deliveryId: string, reason: string): Promise<Delivery> {
    const { delivery } = await this.client.request(
      { type: "close_delivery", delivery_id: deliveryId, reason },
      "delivery_closed",
    );
    return deliveryFromIpc(delivery);
  }

  /** The shell shows the save dialog and writes the engine's chunks; the window names no path. */
  async export(request: MessageExportRequest): Promise<MessageExportResult | null> {
    let saved: { path: string; rows: number } | null;
    try {
      saved = await this.bridge.invoke("save_message_export", {
        suggestedName: `packie-messages-${new Date().toISOString().slice(0, 10)}.${request.format}`,
        ...exportToIpc(request),
      });
    } catch (error) {
      throw fromCommandError(error);
    }
    return saved === null ? null : { savedTo: saved.path, rows: saved.rows };
  }

  async verifyTransmission(query: TransmissionQuery): Promise<TransmissionReport> {
    const report = await this.client.request(
      { type: "verify_transmission", ...transmissionToIpc(query) },
      "transmission",
    );
    return transmissionFromIpc(report);
  }
}
