/**
 * Pins Store
 *
 * Manages user's global pins (notes, links, pinned messages).
 */

import { createSignal } from "solid-js";
import type { Pin, CreatePinRequest, UpdatePinRequest } from "@/lib/types";

// ============================================================================
// State
// ============================================================================

// Detect if running in Tauri
const isTauri = typeof window !== "undefined" && "__TAURI__" in window;

const [pins, setPins] = createSignal<Pin[]>([]);
const [isLoading, setIsLoading] = createSignal(false);

// ============================================================================
// API Calls
// ============================================================================

async function apiCall<T>(
  endpoint: string,
  options?: RequestInit
): Promise<T> {
  // Check if running in Tauri
  if (isTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    const method = options?.method || "GET";
    const body = options?.body ? JSON.parse(options.body as string) : undefined;

    switch (method) {
      case "GET":
        return invoke("fetch_pins") as Promise<T>;
      case "POST":
        return invoke("create_pin", { request: body }) as Promise<T>;
      case "PUT":
        if (endpoint.includes("reorder")) {
          return invoke("reorder_pins", { pinIds: body.pin_ids }) as Promise<T>;
        }
        const pinId = endpoint.split("/").pop();
        return invoke("update_pin", { pinId, request: body }) as Promise<T>;
      case "DELETE":
        const deleteId = endpoint.split("/").pop();
        return invoke("delete_pin", { pinId: deleteId }) as Promise<T>;
      default:
        throw new Error(`Unknown method: ${method}`);
    }
  }

  // HTTP fallback for browser
  const token = localStorage.getItem("vc:token");
  const baseUrl = import.meta.env.VITE_API_URL || "";

  const response = await fetch(`${baseUrl}${endpoint}`, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${token}`,
      ...options?.headers,
    },
  });

  if (!response.ok) {
    throw new Error(`API error: ${response.status}`);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  return response.json();
}

// ============================================================================
// Actions
// ============================================================================

export async function loadPins(): Promise<void> {
  setIsLoading(true);
  try {
    const data = await apiCall<Pin[]>("/api/me/pins");
    setPins(data);
  } catch (error) {
    console.error("Failed to load pins:", error);
  } finally {
    setIsLoading(false);
  }
}

export async function createPin(request: CreatePinRequest): Promise<Pin | null> {
  try {
    const pin = await apiCall<Pin>("/api/me/pins", {
      method: "POST",
      body: JSON.stringify(request),
    });
    setPins((prev) => [...prev, pin]);
    return pin;
  } catch (error) {
    console.error("Failed to create pin:", error);
    return null;
  }
}

export async function updatePin(
  pinId: string,
  request: UpdatePinRequest
): Promise<Pin | null> {
  try {
    const pin = await apiCall<Pin>(`/api/me/pins/${pinId}`, {
      method: "PUT",
      body: JSON.stringify(request),
    });
    setPins((prev) => prev.map((p) => (p.id === pinId ? pin : p)));
    return pin;
  } catch (error) {
    console.error("Failed to update pin:", error);
    return null;
  }
}

export async function deletePin(pinId: string): Promise<boolean> {
  try {
    await apiCall(`/api/me/pins/${pinId}`, { method: "DELETE" });
    setPins((prev) => prev.filter((p) => p.id !== pinId));
    return true;
  } catch (error) {
    console.error("Failed to delete pin:", error);
    return false;
  }
}

export async function reorderPins(pinIds: string[]): Promise<boolean> {
  try {
    await apiCall("/api/me/pins/reorder", {
      method: "PUT",
      body: JSON.stringify({ pin_ids: pinIds }),
    });
    // Reorder local state
    setPins((prev) => {
      const pinMap = new Map(prev.map((p) => [p.id, p]));
      return pinIds
        .map((id, index) => {
          const pin = pinMap.get(id);
          return pin ? { ...pin, position: index } : null;
        })
        .filter((p): p is Pin => p !== null);
    });
    return true;
  } catch (error) {
    console.error("Failed to reorder pins:", error);
    return false;
  }
}

// ============================================================================
// Selectors
// ============================================================================

export { pins, isLoading };

export function getPinsByType(type: Pin["pin_type"]): Pin[] {
  return pins().filter((p) => p.pin_type === type);
}
