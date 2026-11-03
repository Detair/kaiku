import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@solidjs/testing-library";
import SecurityNoticeCard from "../SecurityNoticeCard";
import { event, notice } from "./fixtures";

afterEach(cleanup);

describe("SecurityNoticeCard", () => {
  it("shows the notice, the client, place and ip when present, and Important for high", () => {
    const ev = event({ event_type: "authenticator_added", place: "Hamburg, DE", ip: "203.0.113.7", authenticator: { id: "p1", type: "passkey", label: "YubiKey" } });
    render(() => <SecurityNoticeCard notice={notice({ severity: "high", event_type: "authenticator_added" }, [ev])} meId="me" />);
    expect(screen.getByText("Important")).toBeTruthy();
    expect(screen.getByText(/Kaiku desktop on Windows/)).toBeTruthy();
    expect(screen.getByText(/Hamburg, DE/)).toBeTruthy();
    expect(screen.getByText(/203\.0\.113\.7/)).toBeTruthy();
    expect(screen.getByText("With YubiKey")).toBeTruthy();
  });

  it("truncated notices point to the security log", () => {
    render(() => <SecurityNoticeCard notice={notice({ events_truncated: true })} meId="me" />);
    expect(screen.getByText("And more in your security log.")).toBeTruthy();
  });
});
