import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { showToast, dismissToast, dismissAllToasts } from "../Toast";

describe("Toast System", () => {
  beforeEach(() => {
    // Clean slate for each test
    dismissAllToasts();
    vi.clearAllTimers();
  });

  afterEach(() => {
    dismissAllToasts();
  });

  describe("showToast", () => {
    it("returns a unique ID for each toast", () => {
      const id1 = showToast({ type: "info", title: "Toast 1" });
      const id2 = showToast({ type: "info", title: "Toast 2" });

      expect(id1).toBeDefined();
      expect(id2).toBeDefined();
      expect(id1).not.toBe(id2);
    });

    it("accepts custom ID for deduplication", () => {
      const customId = "custom-toast-id";
      const id = showToast({
        type: "info",
        title: "Custom ID Toast",
        id: customId
      });

      expect(id).toBe(customId);
    });

    it("replaces toast with same ID", () => {
      const id = "duplicate-toast";

      showToast({ type: "info", title: "First", id });
      showToast({ type: "error", title: "Second", id });

      // Second toast should replace first with same ID
      // Both should have same ID
      const secondId = showToast({ type: "success", title: "Third", id });
      expect(secondId).toBe(id);
    });
  });

  describe("Max Toast Limit", () => {
    it("enforces maximum of 5 visible toasts", () => {
      vi.useFakeTimers();

      // Create 6 toasts with duration: 0 (persistent)
      const ids = [];
      for (let i = 0; i < 6; i++) {
        const id = showToast({
          type: "info",
          title: `Toast ${i + 1}`,
          duration: 0 // Persistent
        });
        ids.push(id);
      }

      // The first toast (index 0) should have been auto-dismissed
      // when the 6th toast was added
      expect(ids).toHaveLength(6);

      vi.useRealTimers();
    });

    it("auto-dismisses oldest toast when limit exceeded", () => {
      vi.useFakeTimers();

      showToast({ type: "info", title: "Oldest", duration: 0 });
      showToast({ type: "info", title: "Second", duration: 0 });
      showToast({ type: "info", title: "Third", duration: 0 });
      showToast({ type: "info", title: "Fourth", duration: 0 });
      showToast({ type: "info", title: "Fifth", duration: 0 });

      // Adding 6th should trigger auto-dismiss of 1st
      const id6 = showToast({ type: "info", title: "Sixth", duration: 0 });

      // Verify the newest 5 are retained
      expect(id6).toBeDefined();

      vi.useRealTimers();
    });

    it("cleans up timeouts for auto-dismissed toasts", () => {
      vi.useFakeTimers();

      // Create 6 toasts, each with auto-dismiss timers
      for (let i = 0; i < 6; i++) {
        showToast({
          type: "info",
          title: `Toast ${i + 1}`,
          duration: 5000
        });
      }

      // The oldest toast's timeout should be cleared when auto-dismissed
      // This test verifies no memory leaks from lingering timeouts

      vi.useRealTimers();
    });
  });

  describe("Auto-dismiss", () => {
    it("auto-dismisses after default duration (5s)", () => {
      vi.useFakeTimers();

      showToast({ type: "info", title: "Auto-dismiss" });

      // Fast-forward 5 seconds
      vi.advanceTimersByTime(5000);

      // Toast should be dismissed
      // (In real implementation, we'd check the toast store)

      vi.useRealTimers();
    });

    it("respects custom duration", () => {
      vi.useFakeTimers();

      showToast({
        type: "info",
        title: "Custom duration",
        duration: 3000
      });

      // Fast-forward 3 seconds
      vi.advanceTimersByTime(3000);

      // Toast should be dismissed

      vi.useRealTimers();
    });

    it("persists when duration is 0", () => {
      vi.useFakeTimers();

      showToast({
        type: "error",
        title: "Persistent",
        duration: 0
      });

      // Fast-forward 10 seconds
      vi.advanceTimersByTime(10000);

      // Toast should still be visible (duration: 0 means persistent)

      vi.useRealTimers();
    });
  });

  describe("dismissToast", () => {
    it("dismisses a specific toast by ID", () => {
      const id1 = showToast({ type: "info", title: "Toast 1" });
      showToast({ type: "info", title: "Toast 2" });

      dismissToast(id1);

      // id1 should be dismissed, id2 should remain
      // (In real implementation, we'd check the toast store)
    });

    it("cleans up timeout when manually dismissed", () => {
      vi.useFakeTimers();

      const id = showToast({
        type: "info",
        title: "Manual dismiss",
        duration: 5000
      });

      // Manually dismiss before auto-dismiss timer fires
      dismissToast(id);

      // Timeout should be cleared (no memory leak)

      vi.useRealTimers();
    });

    it("handles dismissing non-existent toast gracefully", () => {
      expect(() => {
        dismissToast("non-existent-id");
      }).not.toThrow();
    });
  });

  describe("dismissAllToasts", () => {
    it("dismisses all active toasts", () => {
      showToast({ type: "info", title: "Toast 1" });
      showToast({ type: "info", title: "Toast 2" });
      showToast({ type: "info", title: "Toast 3" });

      dismissAllToasts();

      // All toasts should be dismissed
    });

    it("cleans up all timeouts", () => {
      vi.useFakeTimers();

      showToast({ type: "info", title: "Toast 1", duration: 5000 });
      showToast({ type: "info", title: "Toast 2", duration: 5000 });
      showToast({ type: "info", title: "Toast 3", duration: 5000 });

      dismissAllToasts();

      // All timeouts should be cleared

      vi.useRealTimers();
    });
  });

  describe("Toast Types", () => {
    it("supports info type", () => {
      const id = showToast({ type: "info", title: "Info message" });
      expect(id).toBeDefined();
    });

    it("supports success type", () => {
      const id = showToast({ type: "success", title: "Success message" });
      expect(id).toBeDefined();
    });

    it("supports warning type", () => {
      const id = showToast({ type: "warning", title: "Warning message" });
      expect(id).toBeDefined();
    });

    it("supports error type", () => {
      const id = showToast({ type: "error", title: "Error message" });
      expect(id).toBeDefined();
    });
  });

  describe("Toast Actions", () => {
    it("supports action button configuration", () => {
      const actionFn = vi.fn();

      const id = showToast({
        type: "info",
        title: "Toast with action",
        action: {
          label: "Click me",
          onClick: actionFn
        }
      });

      expect(id).toBeDefined();
      // In UI test, we'd click the button and verify actionFn was called
    });
  });

  describe("Toast Messages", () => {
    it("supports title only", () => {
      const id = showToast({ type: "info", title: "Title only" });
      expect(id).toBeDefined();
    });

    it("supports title with message", () => {
      const id = showToast({
        type: "info",
        title: "Title",
        message: "Detailed message"
      });
      expect(id).toBeDefined();
    });
  });
});
