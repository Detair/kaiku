/**
 * Webhook Management API
 */

import { getAccessToken } from "../tauri";

const API_BASE = import.meta.env.VITE_API_URL || "http://localhost:3000";

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
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/webhooks`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify(data),
    },
  );

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to create webhook");
  }

  return response.json();
}

/**
 * List webhooks for an application.
 */
export async function listWebhooks(applicationId: string): Promise<Webhook[]> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/webhooks`,
    {
      method: "GET",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to list webhooks");
  }

  return response.json();
}

/**
 * Get a specific webhook.
 */
export async function getWebhook(
  applicationId: string,
  webhookId: string,
): Promise<Webhook> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/webhooks/${webhookId}`,
    {
      method: "GET",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to get webhook");
  }

  return response.json();
}

/**
 * Update a webhook.
 */
export async function updateWebhook(
  applicationId: string,
  webhookId: string,
  data: UpdateWebhookRequest,
): Promise<Webhook> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/webhooks/${webhookId}`,
    {
      method: "PATCH",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${token}`,
      },
      body: JSON.stringify(data),
    },
  );

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to update webhook");
  }

  return response.json();
}

/**
 * Delete a webhook.
 */
export async function deleteWebhook(
  applicationId: string,
  webhookId: string,
): Promise<void> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/webhooks/${webhookId}`,
    {
      method: "DELETE",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to delete webhook");
  }
}

/**
 * Send a test ping to a webhook.
 */
export async function testWebhook(
  applicationId: string,
  webhookId: string,
): Promise<TestDeliveryResult> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/webhooks/${webhookId}/test`,
    {
      method: "POST",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || "Failed to test webhook");
  }

  return response.json();
}

/**
 * List recent delivery log entries for a webhook.
 */
export async function listDeliveries(
  applicationId: string,
  webhookId: string,
): Promise<DeliveryLogEntry[]> {
  const token = getAccessToken();
  const response = await fetch(
    `${API_BASE}/api/applications/${applicationId}/webhooks/${webhookId}/deliveries`,
    {
      method: "GET",
      headers: {
        Authorization: `Bearer ${token}`,
      },
    },
  );

  if (!response.ok) {
    throw new Error("Failed to list deliveries");
  }

  return response.json();
}
