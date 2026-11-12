import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, cleanup } from "@solidjs/testing-library";
import ToastContainer, {
  showToast,
  dismissAllToasts,
  pauseToast,
  resumeToast,
  toasts,
} from "../Toast";

describe("Toast reading time (A11Y-8)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    dismissAllToasts();
  });
  afterEach(() => {
    dismissAllToasts();
    cleanup();
    vi.useRealTimers();
  });

  it("keeps a paused toast past its duration and resumes with the time left", () => {
    const id = showToast({ type: "info", title: "Hi", duration: 5000 });
    vi.advanceTimersByTime(3000);
    pauseToast(id);
    vi.advanceTimersByTime(60_000);
    expect(toasts()).toHaveLength(1);

    resumeToast(id);
    // 2000 ms were left, more than the 1500 ms minimum
    vi.advanceTimersByTime(1999);
    expect(toasts()).toHaveLength(1);
    vi.advanceTimersByTime(2);
    expect(toasts()).toHaveLength(0);
  });

  it("gives a nearly finished toast a short grace time after resuming", () => {
    const id = showToast({ type: "info", title: "Hi", duration: 3000 });
    vi.advanceTimersByTime(2900);
    pauseToast(id);
    resumeToast(id);
    vi.advanceTimersByTime(1400);
    expect(toasts()).toHaveLength(1);
    vi.advanceTimersByTime(200);
    expect(toasts()).toHaveLength(0);
  });

  it("does not start a timer for a persistent toast on resume", () => {
    const id = showToast({ type: "error", title: "Stay", duration: 0 });
    pauseToast(id);
    resumeToast(id);
    vi.advanceTimersByTime(60_000);
    expect(toasts()).toHaveLength(1);
  });

  it("pauses while the pointer is over the toast and resumes when it leaves", () => {
    render(() => <ToastContainer />);
    showToast({ type: "success", title: "Saved", duration: 3000 });
    const alert = screen.getByRole("alert");

    vi.advanceTimersByTime(1000);
    fireEvent.mouseEnter(alert);
    vi.advanceTimersByTime(30_000);
    expect(toasts()).toHaveLength(1);

    fireEvent.mouseLeave(alert);
    vi.advanceTimersByTime(2100);
    expect(toasts()).toHaveLength(0);
  });

  it("pauses while focus is inside the toast and resumes when it leaves", () => {
    render(() => <ToastContainer />);
    showToast({
      type: "info",
      title: "Undo?",
      duration: 3000,
      action: { label: "Undo", onClick: () => {} },
    });
    const alert = screen.getByRole("alert");
    const button = screen.getByRole("button", { name: "Undo" });

    fireEvent.focusIn(button);
    vi.advanceTimersByTime(30_000);
    expect(toasts()).toHaveLength(1);

    // Focus moves to the other button of the same toast: still paused.
    fireEvent.focusOut(button, { relatedTarget: alert.querySelector("button:last-of-type") });
    vi.advanceTimersByTime(30_000);
    expect(toasts()).toHaveLength(1);

    fireEvent.focusOut(button, { relatedTarget: null });
    vi.advanceTimersByTime(3100);
    expect(toasts()).toHaveLength(0);
  });

  it("stays paused while hovered even after focus leaves", () => {
    render(() => <ToastContainer />);
    showToast({
      type: "info",
      title: "Both",
      duration: 3000,
      action: { label: "Go", onClick: () => {} },
    });
    const alert = screen.getByRole("alert");
    const button = screen.getByRole("button", { name: "Go" });
    fireEvent.mouseEnter(alert);
    fireEvent.focusIn(button);
    fireEvent.focusOut(button, { relatedTarget: null });
    vi.advanceTimersByTime(30_000);
    expect(toasts()).toHaveLength(1);
  });

  it("colours every type with theme tokens, no fixed palette colours", () => {
    render(() => <ToastContainer />);
    for (const type of ["info", "success", "warning", "error"] as const) {
      showToast({ type, title: type, duration: 0 });
    }
    const classes = screen
      .getAllByRole("alert")
      .map((el) => el.className)
      .join(" ");
    expect(classes).not.toMatch(/\b(?:bg|border|text)-(?:blue|green|yellow|red|white)\b|-(?:500|600)\b/);
    expect(classes).toContain("text-on-warning");
    expect(classes).toContain("text-on-success");
    expect(classes).toContain("text-on-danger");
    expect(classes).toContain("text-on-accent");
  });
});
