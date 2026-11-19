import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";
import SessionExpiredModal from "../SessionExpiredModal";
import Modal from "@/components/ui/Modal";
import { MODAL_LAYER } from "@/lib/a11y/modalLayer";
import { setAuthState } from "@/stores/auth";

afterEach(() => {
  cleanup();
  setAuthState({ sessionExpired: false, sessionEndReason: "expired" });
});

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

describe("SessionExpiredModal layer", () => {
  it("shows in front of a window opened after the app started (Log in stays reachable)", () => {
    // Mounted with the app, long before the window opens.
    render(() => (
      <Router>
        <Route path="/" component={SessionExpiredModal} />
      </Router>
    ));
    render(() => (
      <Modal onClose={() => {}} label="Earlier window" layer={MODAL_LAYER.prompt}>
        <button type="button">inside</button>
      </Modal>
    ));
    const earlier = screen.getByRole("dialog", { name: "Earlier window" });

    setAuthState({ sessionExpired: true, sessionEndReason: "expired" });
    const logIn = screen.getByRole("button", { name: "Log in", hidden: true });
    expect(zOf(logIn)).toBeGreaterThan(zOf(earlier));
  });

  it("takes the keyboard from the window below: focus on Log in, Tab stays, Escape changes nothing", async () => {
    render(() => (
      <Router>
        <Route path="/" component={SessionExpiredModal} />
      </Router>
    ));
    const closeBelow = vi.fn();
    render(() => (
      <Modal onClose={closeBelow} label="Earlier window">
        <button type="button">inside</button>
      </Modal>
    ));
    await new Promise((r) => setTimeout(r, 0));
    const below = screen.getByRole("dialog", { name: "Earlier window" });
    expect(below.contains(document.activeElement)).toBe(true);

    setAuthState({ sessionExpired: true, sessionEndReason: "expired" });
    await new Promise((r) => setTimeout(r, 0));
    const logIn = screen.getByRole("button", { name: "Log in", hidden: true });
    expect(document.activeElement).toBe(logIn);

    fireEvent.keyDown(logIn, { key: "Tab" });
    expect(document.activeElement).toBe(logIn);
    fireEvent.keyDown(logIn, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(logIn);

    fireEvent.keyDown(logIn, { key: "Escape" });
    expect(closeBelow).not.toHaveBeenCalled();
    expect(logIn.isConnected).toBe(true);
    expect(below.isConnected).toBe(true);
  });
});
