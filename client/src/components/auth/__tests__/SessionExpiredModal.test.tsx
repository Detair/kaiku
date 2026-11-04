import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";
import { Router, Route } from "@solidjs/router";
import SessionExpiredModal from "../SessionExpiredModal";
import { setAuthState } from "@/stores/auth";

afterEach(() => {
  cleanup();
  setAuthState({ sessionExpired: false, sessionEndReason: "expired" });
});

describe("SessionExpiredModal", () => {
  it("SESSION_REUSED shows the key-used-twice text", () => {
    setAuthState({ sessionExpired: true, sessionEndReason: "reused" });
    render(() => (
      <Router>
        <Route path="/" component={SessionExpiredModal} />
      </Router>
    ));
    expect(
      screen.getByText("A session was ended because its key was used twice. Sign in again to continue."),
    ).toBeTruthy();
  });
});
