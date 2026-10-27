/**
 * Webhook Management API
 */

import { httpRequest } from "../tauri/common";

export type WebhookEventType =
  | "message.created"
  | "member.joined"
  | "member.left"
  | "command.invoked";

export interface Webhook {
  id: string;
  application_id: string;
  url: string;
  subscribed_events: WebhookEventType[];
  active: boolean;
  description?: string;
  created_at: string;
  updated_at: string;
}

export interface WebhookCreated extends Webhook {
  signing_secret: string;
}

export interface CreateWebhookRequest {
  url: string;
  subscribed_events: WebhookEventType[];
  description?: string;
}

export interface UpdateWebhookRequest {
  url?: string;
  subscribed_events?: WebhookEventType[];
  active?: boolean;
  description?: string;
}

export interface DeliveryLogEntry {
  id: string;
  webhook_id: string;
  event_type: WebhookEventType;
  event_id: string;
  response_status?: number;
  success: boolean;
  attempt: number;
  error_message?: string;
  latency_ms?: number;
  created_at: string;
}

export interface TestDeliveryResult {
  success: boolean;
  response_status?: number;
  latency_ms: number;
  error_message?: string;
}

/**
 * Create a new webhook for an application.
 * **The signing_secret is only returned once.**
 */
export async function createWebhook(
  applicationId: string,
  data: CreateWebhookRequest,
): Promise<WebhookCreated> {
  return httpRequest<WebhookCreated>(
    "POST",
    `/api/applications/${applicationId}/webhooks`,
    data,
  );
}

/**
 * List webhooks for an application.
 */
export async function listWebhooks(applicationId: string): Promise<Webhook[]> {
  return httpRequest<Webhook[]>(
    "GET",
    `/api/applications/${applicationId}/webhooks`,
  );
}

/**
 * Get a specific webhook.
 */
export async function getWebhook(
  applicationId: string,
  webhookId: string,
): Promise<Webhook> {
  return httpRequest<Webhook>(
    "GET",
    `/api/applications/${applicationId}/webhooks/${webhookId}`,
  );
}

/**
 * Update a webhook.
 */
export async function updateWebhook(
  applicationId: string,
  webhookId: string,
  data: UpdateWebhookRequest,
): Promise<Webhook> {
  return httpRequest<Webhook>(
    "PATCH",
    `/api/applications/${applicationId}/webhooks/${webhookId}`,
    data,
  );
}

/**
 * Delete a webhook.
 */
export async function deleteWebhook(
  applicationId: string,
  webhookId: string,
): Promise<void> {
  await httpRequest<void>(
    "DELETE",
    `/api/applications/${applicationId}/webhooks/${webhookId}`,
  );
}

/**
 * Send a test ping to a webhook.
 */
export async function testWebhook(
  applicationId: string,
  webhookId: string,
): Promise<TestDeliveryResult> {
  return httpRequest<TestDeliveryResult>(
    "POST",
    `/api/applications/${applicationId}/webhooks/${webhookId}/test`,
  );
}

/**
 * List recent delivery log entries for a webhook.
 */
export async function listDeliveries(
  applicationId: string,
  webhookId: string,
): Promise<DeliveryLogEntry[]> {
  return httpRequest<DeliveryLogEntry[]>(
    "GET",
    `/api/applications/${applicationId}/webhooks/${webhookId}/deliveries`,
  );
}
