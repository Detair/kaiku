import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor, fireEvent } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import QrLoginPanel from "../QrLoginPanel";
import AttemptEnded from "../AttemptEnded";
import ConfirmationPanel from "../ConfirmationPanel";
import TotpStepPanel from "../TotpStepPanel";
import type { LoginAttempt } from "@/lib/types";

function attempt(over: Partial<LoginAttempt> = {}): LoginAttempt {
  return {
    attempt_id: "44444444-4444-4444-8444-444444444444",
    kind: "qr",
    state: "open",
    expires_at: new Date(Date.now() + 9.5 * 60_000).toISOString(),
    qr: null,
    totp_available: false,
    ...over,
  };
}

describe("QrLoginPanel", () => {
  it("shows the heading, three steps, the code and the minutes left", async () => {
    render(() => <QrLoginPanel attempt={attempt()} payload="kaiku://login/v1?o=x&t=one" onBack={() => {}} />);
    expect(screen.getByRole("heading", { name: "Sign in with your phone" })).toBeTruthy();
    expect(screen.getByText("Open Kaiku on your phone")).toBeTruthy();
    expect(screen.getByText("Tap Scan")).toBeTruthy();
    expect(screen.getByText("Point it at this code")).toBeTruthy();
    const img = await screen.findByAltText("Sign-in code for the Kaiku app on your phone");
    expect(img.getAttribute("src")).toMatch(/^data:image\/svg\+xml/);
    expect(screen.getByText("This sign-in is open for 10 more minutes.")).toBeTruthy();
  });

  it("QR rotates when a poll returns a new payload", async () => {
    const [payload, setPayload] = createSignal("kaiku://login/v1?o=x&t=one");
    render(() => <QrLoginPanel attempt={attempt()} payload={payload()} onBack={() => {}} />);
    const img = await screen.findByAltText("Sign-in code for the Kaiku app on your phone");
    await waitFor(() => expect(img.getAttribute("src")).toBeTruthy());
    const first = img.getAttribute("src");
    setPayload("kaiku://login/v1?o=x&t=two");
    await waitFor(() => expect(img.getAttribute("src")).not.toBe(first));
  });

  it("no typed sign-in code is offered", () => {
    render(() => <QrLoginPanel attempt={attempt()} payload="kaiku://login/v1?o=x&t=one" onBack={() => {}} />);
    expect(document.querySelector("input")).toBeNull();
  });

  it("offers a passkey only when the page passes one", () => {
    const usePasskey = vi.fn();
    render(() => (
      <QrLoginPanel attempt={attempt()} payload="p" onBack={() => {}} onUsePasskey={usePasskey} />
    ));
    fireEvent.click(screen.getByRole("button", { name: "Use a passkey instead" }));
    expect(usePasskey).toHaveBeenCalled();
  });
});

describe("ConfirmationPanel", () => {
  it("shows the F3 copy and Phone not at hand only with TOTP", () => {
    const notAtHand = vi.fn();
    const { unmount } = render(() => (
      <ConfirmationPanel attempt={attempt({ kind: "password", totp_available: true })} payload="p" via="password" onPhoneNotAtHand={notAtHand} onBack={() => {}} />
    ));
    expect(screen.getByText("Open Kaiku on your phone and scan this code.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Phone not at hand?" }));
    expect(notAtHand).toHaveBeenCalled();
    unmount();
    render(() => (
      <ConfirmationPanel attempt={attempt({ kind: "password" })} payload="p" via="password" onPhoneNotAtHand={notAtHand} onBack={() => {}} />
    ));
    expect(screen.queryByRole("button", { name: "Phone not at hand?" })).toBeNull();
  });
});

describe("TotpStepPanel", () => {
  it("has a one-time-code field, the reuse hint and submits the digits", () => {
    const submit = vi.fn();
    render(() => <TotpStepPanel error={null} busy={false} onSubmit={submit} onBack={() => {}} />);
    const input = screen.getByLabelText("Code from your authenticator app") as HTMLInputElement;
    expect(input.getAttribute("autocomplete")).toBe("one-time-code");
    expect(screen.getByText("If you just used a code, wait for the next one.")).toBeTruthy();
    fireEvent.input(input, { target: { value: "123 456" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign in" }));
    expect(submit).toHaveBeenCalledWith("123456");
  });
});

describe("AttemptEnded", () => {
  it("denied, reported and expired show AttemptEnded with Try again", () => {
    const texts: Record<string, string> = {
      denied: "Your phone said no. Nothing was signed in.",
      reported: "Your phone reported this sign-in. Nothing was signed in, and your account was warned.",
      cancelled: "This sign-in was cancelled.",
      expired: "This code ran out. Get a new one to try again.",
    };
    for (const [state, text] of Object.entries(texts)) {
      const retry = vi.fn();
      const { unmount } = render(() => (
        <AttemptEnded state={state as "denied"} via="qr" onRetry={retry} onBack={() => {}} />
      ));
      expect(screen.getByText(text)).toBeTruthy();
      fireEvent.click(screen.getByRole("button", { name: "Try again" }));
      expect(retry).toHaveBeenCalled();
      unmount();
    }
  });
});
