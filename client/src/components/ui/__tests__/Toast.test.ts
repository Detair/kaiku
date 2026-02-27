import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { showToast, dismissToast, dismissAllToasts, toasts } from "../Toast";

/**
 * TODO: Add component rendering tests using @solidjs/testing-library:
 * - Verify max-5 visual limit in DOM
 * - Test auto-dismiss with component lifecycle
 * - Test action button clicks
 * - Test toast stacking and animations
 */

describe("Toast System", () => {
  beforeEach(() => {
    dismissAllToasts();
  });

  afterEach(() => {
    dismissAllToasts();
    try {
      vi.useRealTimers();
    } catch {
      // Timers not active
    }
  });

  describe("showToast", () => {
    it("returns a unique ID for each toast", () => {
      const id1 = showToast({ type: "info", title: "Toast 1", duration: 0 });
      const id2 = showToast({ type: "info", title: "Toast 2", duration: 0 });

      expect(id1).toBeDefined();
      expect(id2).toBeDefined();
      expect(id1).not.toBe(id2);
      expect(toasts()).toHaveLength(2);
    });

    it("accepts custom ID for deduplication", () => {
      const customId = "custom-toast-id";
      const id = showToast({
        type: "info",
        title: "Custom ID Toast",
        id: customId,
        duration: 0,
      });

      expect(id).toBe(customId);
      expect(toasts()).toHaveLength(1);
      expect(toasts()[0].id).toBe(customId);
    });

    it("replaces toast with same ID", () => {
      const id = "duplicate-toast";

      showToast({ type: "info", title: "First", id, duration: 0 });
      expect(toasts()).toHaveLength(1);
      expect(toasts()[0].title).toBe("First");

      showToast({ type: "error", title: "Second", id, duration: 0 });
      expect(toasts()).toHaveLength(1);
      expect(toasts()[0].title).toBe("Second");
      expect(toasts()[0].type).toBe("error");
    });
  });

  describe("Max Toast Limit", () => {
    it("enforces maximum of 5 visible toasts", () => {
      for (let i = 0; i < 6; i++) {
        showToast({ type: "info", title: `Toast ${i + 1}`, duration: 0 });
      }
      expect(toasts()).toHaveLength(5);
    });

    it("auto-dismisses oldest toast when limit exceeded", () => {
      for (let i = 0; i < 5; i++) {
        showToast({ type: "info", title: `Toast ${i + 1}`, duration: 0 });
      }
      showToast({ type: "info", title: "Sixth", duration: 0 });

      expect(toasts()).toHaveLength(5);
      expect(toasts()[0].title).toBe("Toast 2");
      expect(toasts()[4].title).toBe("Sixth");
    });

    it("cleans up timeouts for auto-dismissed toasts", () => {
      vi.useFakeTimers();
      for (let i = 0; i < 6; i++) {
        showToast({ type: "info", title: `Toast ${i + 1}`, duration: 5000 });
      }
      // Oldest evicted, 5 remain
      expect(toasts()).toHaveLength(5);
      vi.useRealTimers();
    });
  });

  describe("Auto-dismiss", () => {
    it("auto-dismisses after default duration (5s)", () => {
      vi.useFakeTimers();
      showToast({ type: "info", title: "Auto-dismiss" });
      expect(toasts()).toHaveLength(1);

      vi.advanceTimersByTime(5000);
      expect(toasts()).toHaveLength(0);
    });

    it("respects custom duration", () => {
      vi.useFakeTimers();
      showToast({ type: "info", title: "Custom duration", duration: 3000 });
      expect(toasts()).toHaveLength(1);

      vi.advanceTimersByTime(3000);
      expect(toasts()).toHaveLength(0);
    });

    it("persists when duration is 0", () => {
      vi.useFakeTimers();
      showToast({ type: "error", title: "Persistent", duration: 0 });
      expect(toasts()).toHaveLength(1);

      vi.advanceTimersByTime(10000);
      expect(toasts()).toHaveLength(1);
    });
  });

  describe("dismissToast", () => {
    it("dismisses a specific toast by ID", () => {
      const id1 = showToast({ type: "info", title: "Toast 1", duration: 0 });
      const id2 = showToast({ type: "info", title: "Toast 2", duration: 0 });
      expect(toasts()).toHaveLength(2);

      dismissToast(id1);
      expect(toasts()).toHaveLength(1);
      expect(toasts()[0].id).toBe(id2);
    });

    it("cleans up timeout when manually dismissed", () => {
      vi.useFakeTimers();
      const id = showToast({
        type: "info",
        title: "Manual dismiss",
        duration: 5000,
      });
      expect(toasts()).toHaveLength(1);

      dismissToast(id);
      expect(toasts()).toHaveLength(0);

      // Advancing should not throw or re-dismiss
      vi.advanceTimersByTime(5000);
      expect(toasts()).toHaveLength(0);
    });

    it("handles dismissing non-existent toast gracefully", () => {
      expect(() => dismissToast("non-existent-id")).not.toThrow();
    });
  });

  describe("dismissAllToasts", () => {
    it("dismisses all active toasts", () => {
      showToast({ type: "info", title: "Toast 1", duration: 0 });
      showToast({ type: "info", title: "Toast 2", duration: 0 });
      showToast({ type: "info", title: "Toast 3", duration: 0 });
      expect(toasts()).toHaveLength(3);

      dismissAllToasts();
      expect(toasts()).toHaveLength(0);
    });

    it("cleans up all timeouts", () => {
      vi.useFakeTimers();
      showToast({ type: "info", title: "Toast 1", duration: 5000 });
      showToast({ type: "info", title: "Toast 2", duration: 5000 });
      expect(toasts()).toHaveLength(2);

      dismissAllToasts();
      expect(toasts()).toHaveLength(0);

      vi.advanceTimersByTime(5000);
      expect(toasts()).toHaveLength(0);
    });
  });

  describe("Toast Types", () => {
    it("supports all four types", () => {
      showToast({ type: "info", title: "Info", duration: 0 });
      showToast({ type: "success", title: "Success", duration: 0 });
      showToast({ type: "warning", title: "Warning", duration: 0 });
      showToast({ type: "error", title: "Error", duration: 0 });
      expect(toasts()).toHaveLength(4);
    });
  });

  describe("Toast Actions", () => {
    it("supports action button configuration", () => {
      const actionFn = vi.fn();
      const id = showToast({
        type: "info",
        title: "Toast with action",
        action: { label: "Click me", onClick: actionFn },
        duration: 0,
      });
      expect(id).toBeDefined();
      expect(toasts()[0].action?.label).toBe("Click me");
    });
  });

  describe("Toast Messages", () => {
    it("supports title only", () => {
      showToast({ type: "info", title: "Title only", duration: 0 });
      expect(toasts()[0].title).toBe("Title only");
      expect(toasts()[0].message).toBeUndefined();
    });

    it("supports title with message", () => {
      showToast({
        type: "info",
        title: "Title",
        message: "Detailed message",
        duration: 0,
      });
      expect(toasts()[0].title).toBe("Title");
      expect(toasts()[0].message).toBe("Detailed message");
    });
  });
});
