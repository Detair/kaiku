/**
 * Pins Store
 *
 * Manages user's global pins (notes, links, pinned messages).
 */

import { createSignal } from "solid-js";
import type { Pin, CreatePinRequest, UpdatePinRequest } from "@/lib/types";
import * as tauri from "@/lib/tauri";

// ============================================================================
// State
// ============================================================================

const [pins, setPins] = createSignal<Pin[]>([]);
const [isLoading, setIsLoading] = createSignal(false);

// ============================================================================
// Actions
// ============================================================================

export async function loadPins(): Promise<void> {
  setIsLoading(true);
  try {
    const data = await tauri.fetchPins();
    setPins(data);
  } catch (error) {
    console.error("Failed to load pins:", error);
  } finally {
    setIsLoading(false);
  }
}

export async function createPin(
  request: CreatePinRequest,
): Promise<Pin | null> {
  try {
    const pin = await tauri.createPin(request);
    setPins((prev) => [...prev, pin]);
    return pin;
  } catch (error) {
    console.error("Failed to create pin:", error);
    return null;
  }
}

export async function updatePin(
  pinId: string,
  request: UpdatePinRequest,
): Promise<Pin | null> {
  try {
    const pin = await tauri.updatePin(pinId, request);
    setPins((prev) => prev.map((p) => (p.id === pinId ? pin : p)));
    return pin;
  } catch (error) {
    console.error("Failed to update pin:", error);
    return null;
  }
}

export async function deletePin(pinId: string): Promise<boolean> {
  try {
    await tauri.deletePin(pinId);
    setPins((prev) => prev.filter((p) => p.id !== pinId));
    return true;
  } catch (error) {
    console.error("Failed to delete pin:", error);
    return false;
  }
}

export async function reorderPins(pinIds: string[]): Promise<boolean> {
  try {
    await tauri.reorderPins(pinIds);
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
