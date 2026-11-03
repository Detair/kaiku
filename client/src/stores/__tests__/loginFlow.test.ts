import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";

const api = vi.hoisted(() => ({
  createQrAttempt: vi.fn(),
  pollAttempt: vi.fn(),
  collectAttempt: vi.fn(),
  cancelAttempt: vi.fn(),
  attemptTotp: vi.fn(),
  discardAdoptedSession: vi.fn(),
}));
/** The real choice semantics: a new choice makes the previous one stale. */
const choices = vi.hoisted(() => {
  let ctl: AbortController | null = null;
  return () => {
    ctl?.abort();
    const mine = new AbortController();
    ctl = mine;
    return { signal: mine.signal, isCurrent: () => !mine.signal.aborted };
  };
});
const authStore = vi.hoisted(() => ({
  login: vi.fn(),
  completeSignIn: vi.fn(),
  forgetStepPassword: vi.fn(),
  beginSignInChoice: vi.fn(),
}));
vi.mock("@/lib/tauri", () => api);
vi.mock("@/stores/auth", () => authStore);

import {
  applyLoginStep,
  leaveFlow,
  loginErrorText,
  loginFlow,
  loginFlowError,
  retryLoginFlow,
  setFlowServer,
  startPhoneSignIn,
  submitPassword,
  submitTotp,
  usePhoneNotAtHand,
} from "../loginFlow";
import { HttpError } from "@/lib/tauri/common";
import type { LoginAttempt } from "@/lib/types";

const ID = "44444444-4444-4444-8444-444444444444";

function attempt(over: Partial<LoginAttempt> = {}): LoginAttempt {
  return {
    attempt_id: ID,
    kind: "qr",
    state: "open",
    expires_at: new Date(Date.now() + 10 * 60_000).toISOString(),
    qr: { payload: "kaiku://login/v1?o=x&t=one", expires_at: "" },
    totp_available: false,
    ...over,
  };
}

/** A poll that stays pending until the test resolves it. */
function pendingPolls() {
  const resolvers: ((a: LoginAttempt) => void)[] = [];
  const rejecters: ((e: unknown) => void)[] = [];
  api.pollAttempt.mockImplementation(
    () =>
      new Promise<LoginAttempt>((resolve, reject) => {
        resolvers.push(resolve);
        rejecters.push(reject);
      }),
  );
  return {
    count: () => resolvers.length,
    resolve: (a: LoginAttempt) => resolvers[resolvers.length - 1](a),
    reject: (e: unknown) => rejecters[rejecters.length - 1](e),
  };
}

beforeEach(() => {
  vi.useFakeTimers();
  for (const f of [...Object.values(api), ...Object.values(authStore)]) f.mockReset();
  api.cancelAttempt.mockResolvedValue(undefined);
  api.discardAdoptedSession.mockResolvedValue(undefined);
  authStore.completeSignIn.mockResolvedValue({ id: "u1" });
  authStore.beginSignInChoice.mockImplementation(choices);
});

afterEach(async () => {
  await leaveFlow();
  vi.useRealTimers();
});

describe("login flow store", () => {
  it("password login shows the confirmation QR on confirmation_required", async () => {
    pendingPolls();
    authStore.login.mockResolvedValue({
      status: "confirmation_required",
      attempt: attempt({ kind: "password", totp_available: true }),
    });
    await submitPassword("https://chat.example.com", "alice", "pw");
    const f = loginFlow();
    expect(f.step).toBe("confirm");
    if (f.step !== "confirm") return;
    expect(f.via).toBe("password");
    expect(f.qrPayload).toBe("kaiku://login/v1?o=x&t=one");
    expect(api.pollAttempt).toHaveBeenCalledWith(ID, 25);
  });

  it("totp_required goes straight to the code step", async () => {
    authStore.login.mockResolvedValue({ status: "totp_required", attempt: attempt({ kind: "password" }) });
    await submitPassword("https://chat.example.com", "alice", "pw");
    expect(loginFlow()).toMatchObject({ step: "totp", from: "totp_required" });
    expect(api.pollAttempt).not.toHaveBeenCalled();
  });

  it("QR rotates when a poll returns a new payload", async () => {
    const polls = pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt());
    await startPhoneSignIn("https://chat.example.com");
    polls.resolve(attempt({ qr: { payload: "kaiku://login/v1?o=x&t=two", expires_at: "" } }));
    await vi.advanceTimersByTimeAsync(1000);
    const f = loginFlow();
    expect(f).toMatchObject({ step: "qr", qrPayload: "kaiku://login/v1?o=x&t=two" });
    if (f.step === "qr") expect(f.attempt.attempt_id).toBe(ID);
    // A poll without a new token keeps the shown code.
    polls.resolve(attempt({ qr: null }));
    await vi.advanceTimersByTimeAsync(1000);
    expect(loginFlow()).toMatchObject({ qrPayload: "kaiku://login/v1?o=x&t=two" });
    expect(api.createQrAttempt).toHaveBeenCalledTimes(1);
  });

  it("approved attempt is collected exactly once and signs in", async () => {
    const polls = pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt());
    api.collectAttempt.mockResolvedValue({ user: { id: "u1" }, setup_required: false, password_change_required: false });
    await startPhoneSignIn("https://chat.example.com");
    polls.resolve(attempt({ state: "approved", qr: null }));
    await vi.advanceTimersByTimeAsync(5000);
    expect(api.collectAttempt).toHaveBeenCalledTimes(1);
    expect(api.collectAttempt).toHaveBeenCalledWith(ID, expect.any(Function));
    expect(authStore.completeSignIn).toHaveBeenCalledTimes(1);
    expect(loginFlow().step).toBe("form");
    expect(polls.count()).toBe(1);
  });

  it("denied, reported and expired end the flow", async () => {
    for (const state of ["denied", "reported", "cancelled", "expired"] as const) {
      const polls = pendingPolls();
      api.createQrAttempt.mockResolvedValue(attempt());
      await startPhoneSignIn("https://chat.example.com");
      polls.resolve(attempt({ state, qr: null }));
      await vi.advanceTimersByTimeAsync(10);
      expect(loginFlow()).toEqual({ step: "ended", state, via: "qr" });
    }
  });

  it("expired attempt offers a new code", async () => {
    pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt({ expires_at: new Date(Date.now() + 1000).toISOString() }));
    await startPhoneSignIn("https://chat.example.com");
    // The long-poll never answers; the attempt runs out on the PC's clock.
    await vi.advanceTimersByTimeAsync(31_000);
    expect(loginFlow()).toEqual({ step: "ended", state: "expired", via: "qr" });
    api.createQrAttempt.mockResolvedValue(attempt({ attempt_id: "55555555-5555-4555-8555-555555555555" }));
    await retryLoginFlow();
    expect(api.createQrAttempt).toHaveBeenCalledTimes(2);
    expect(loginFlow().step).toBe("qr");
  });

  it("an attempt far in the future does not end at once", async () => {
    pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt({ expires_at: "2099-01-01T00:00:00Z" }));
    await startPhoneSignIn("https://chat.example.com");
    await vi.advanceTimersByTimeAsync(5000);
    expect(loginFlow().step).toBe("qr");
  });

  it("410 LOGIN_ATTEMPT_ENDED ends with the reported state", async () => {
    const polls = pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt());
    await startPhoneSignIn("https://chat.example.com");
    polls.reject(new HttpError(410, "x", "LOGIN_ATTEMPT_ENDED", { state: "denied" }));
    await vi.advanceTimersByTimeAsync(10);
    expect(loginFlow()).toEqual({ step: "ended", state: "denied", via: "qr" });
  });

  it("network error while polling retries and keeps the attempt", async () => {
    const polls = pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt());
    await startPhoneSignIn("https://chat.example.com");
    polls.reject(new TypeError("Failed to fetch"));
    await vi.advanceTimersByTimeAsync(500);
    expect(polls.count()).toBe(1);
    await vi.advanceTimersByTimeAsync(600); // 1 s after the first failure
    expect(polls.count()).toBe(2);
    polls.reject(new TypeError("Failed to fetch"));
    await vi.advanceTimersByTimeAsync(1999);
    expect(polls.count()).toBe(2);
    await vi.advanceTimersByTimeAsync(2); // 2 s after the second
    expect(polls.count()).toBe(3);
    polls.reject(new HttpError(503, "x", "SERVICE_UNAVAILABLE"));
    await vi.advanceTimersByTimeAsync(5001); // then every 5 s
    expect(polls.count()).toBe(4);
    expect(loginFlow()).toMatchObject({ step: "qr" });
    expect(api.createQrAttempt).toHaveBeenCalledTimes(1);
    expect(api.pollAttempt.mock.calls.every(([id]) => id === ID)).toBe(true);
  });

  it("leaving the screen cancels the attempt", async () => {
    pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt());
    await startPhoneSignIn("https://chat.example.com");
    await leaveFlow();
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
    expect(loginFlow().step).toBe("form");
  });

  it("phone not at hand switches to the TOTP step without a new attempt", async () => {
    pendingPolls();
    authStore.login.mockResolvedValue({
      status: "confirmation_required",
      attempt: attempt({ kind: "password", totp_available: true }),
    });
    await submitPassword("https://chat.example.com", "alice", "pw");
    usePhoneNotAtHand();
    expect(loginFlow()).toMatchObject({ step: "totp", from: "phone_not_at_hand" });
    const f = loginFlow();
    if (f.step === "totp") expect(f.attempt.attempt_id).toBe(ID);
    expect(api.createQrAttempt).not.toHaveBeenCalled();
    expect(api.cancelAttempt).not.toHaveBeenCalled();
  });

  it("phone not at hand is refused without TOTP", async () => {
    pendingPolls();
    applyLoginStep({ status: "confirmation_required", attempt: attempt({ kind: "password" }) }, "password");
    usePhoneNotAtHand();
    expect(loginFlow().step).toBe("confirm");
  });

  it("INVALID_MFA keeps the TOTP step", async () => {
    applyLoginStep({ status: "totp_required", attempt: attempt({ kind: "password" }) }, "password");
    api.attemptTotp.mockRejectedValueOnce(new HttpError(401, "x", "INVALID_MFA"));
    await expect(submitTotp("123456")).rejects.toBeTruthy();
    expect(loginFlow().step).toBe("totp");
    expect(loginFlowError()).toBe(
      "That code didn't work. If you just used it, wait for the next code and try again.",
    );
    api.attemptTotp.mockResolvedValueOnce({ user: { id: "u1" }, setup_required: false, password_change_required: false });
    await submitTotp("654321");
    expect(api.attemptTotp).toHaveBeenLastCalledWith(ID, "654321", expect.any(Function));
    expect(authStore.completeSignIn).toHaveBeenCalledTimes(1);
    expect(loginFlow().step).toBe("form");
  });

  it("TOO_MANY_ATTEMPTS shows the wait from Retry-After", () => {
    expect(loginErrorText(new HttpError(429, "x", "TOO_MANY_ATTEMPTS", {}, 120))).toBe(
      "Too many tries. Try again in 2 minutes.",
    );
    expect(loginErrorText(new HttpError(429, "x", "TOO_MANY_ATTEMPTS", {}, 30))).toBe(
      "Too many tries. Try again in 1 minute.",
    );
    expect(loginErrorText(new HttpError(429, "x", "TOO_MANY_ATTEMPTS"))).toBe("Too many tries. Try again later.");
  });

  it("neutral answer uses the one F3 text for INVALID_CREDENTIALS", () => {
    expect(loginErrorText(new HttpError(401, "Invalid credentials", "INVALID_CREDENTIALS"))).toBe(
      "Wrong username or password, or password sign-in is not available for this account. Use a passkey or 'Sign in with phone', or ask an admin.",
    );
  });

  it("a collect that outlives the attempt's expiry still signs in", async () => {
    pendingPolls();
    let finishCollect!: (r: unknown) => void;
    api.collectAttempt.mockImplementation(() => new Promise((r) => (finishCollect = r)));
    api.createQrAttempt.mockResolvedValue(attempt({ expires_at: new Date(Date.now() + 2000).toISOString() }));
    api.pollAttempt.mockResolvedValueOnce(attempt({ state: "approved", qr: null }));
    await startPhoneSignIn("https://chat.example.com");
    await vi.advanceTimersByTimeAsync(10);
    expect(api.collectAttempt).toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(5000);
    finishCollect({ user: { id: "u1" }, setup_required: false, password_change_required: false });
    await vi.advanceTimersByTimeAsync(10);
    expect(authStore.completeSignIn).toHaveBeenCalledTimes(1);
  });

  it("leaving during the password call cancels the attempt it returns", async () => {
    let answer!: (s: unknown) => void;
    authStore.login.mockImplementation(() => new Promise((r) => (answer = r)));
    const pending = submitPassword("https://chat.example.com", "alice", "pw");
    await vi.advanceTimersByTimeAsync(0);
    await leaveFlow();
    answer({ status: "confirmation_required", attempt: attempt({ kind: "password" }) });
    await pending;
    expect(loginFlow().step).toBe("form");
    expect(api.pollAttempt).not.toHaveBeenCalled();
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
  });
});

describe("login flow ownership (Codex review)", () => {
  const signedIn = { user: { id: "u1" }, setup_required: false, password_change_required: false };

  /** A QR attempt on `url` whose poll says approved, with the collect held open. */
  async function approvedWithPendingCollect(url = "https://chat.example.com") {
    let finish!: (r: unknown) => void;
    let fail!: (e: unknown) => void;
    api.collectAttempt.mockImplementation(
      () =>
        new Promise((resolve, reject) => {
          finish = resolve;
          fail = reject;
        }),
    );
    api.createQrAttempt.mockResolvedValue(attempt());
    api.pollAttempt.mockResolvedValueOnce(attempt({ state: "approved", qr: null }));
    api.pollAttempt.mockImplementation(() => new Promise(() => undefined));
    await startPhoneSignIn(url);
    await vi.advanceTimersByTimeAsync(10);
    expect(api.collectAttempt).toHaveBeenCalledTimes(1);
    return { finish: (r: unknown) => finish(r), fail: (e: unknown) => fail(e) };
  }

  it("leaving while a collect is in flight never signs in and ends that session", async () => {
    const collect = await approvedWithPendingCollect();
    const stillWanted = api.collectAttempt.mock.calls[0][1] as () => boolean;
    expect(stillWanted()).toBe(true);
    await leaveFlow();
    expect(stillWanted()).toBe(false);
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
    collect.finish(signedIn);
    await vi.advanceTimersByTimeAsync(10);
    expect(authStore.completeSignIn).not.toHaveBeenCalled();
    expect(api.discardAdoptedSession).toHaveBeenCalledWith(signedIn);
    expect(loginFlow().step).toBe("form");
  });

  it("a late collect never replaces the flow that followed it", async () => {
    const collect = await approvedWithPendingCollect();
    const second = attempt({ attempt_id: "55555555-5555-4555-8555-555555555555" });
    api.createQrAttempt.mockResolvedValue(second);
    await startPhoneSignIn("https://chat.example.com");
    collect.finish(signedIn);
    await vi.advanceTimersByTimeAsync(10);
    expect(authStore.completeSignIn).not.toHaveBeenCalled();
    expect(loginFlow()).toMatchObject({ step: "qr", attempt: { attempt_id: second.attempt_id } });
  });

  it("a late TOTP success after leaving neither signs in nor stops the new flow", async () => {
    applyLoginStep({ status: "totp_required", attempt: attempt({ kind: "password" }) }, "password");
    let finish!: (r: unknown) => void;
    api.attemptTotp.mockImplementation(() => new Promise((r) => (finish = r)));
    const pending = submitTotp("123456");
    expect(api.attemptTotp).toHaveBeenCalledWith(ID, "123456", expect.any(Function));
    pendingPolls();
    const second = attempt({ attempt_id: "55555555-5555-4555-8555-555555555555" });
    api.createQrAttempt.mockResolvedValue(second);
    await startPhoneSignIn("https://chat.example.com");
    finish(signedIn);
    await expect(pending).rejects.toBeTruthy();
    expect(authStore.completeSignIn).not.toHaveBeenCalled();
    expect(api.discardAdoptedSession).toHaveBeenCalledWith(signedIn);
    expect(loginFlow()).toMatchObject({ step: "qr", attempt: { attempt_id: second.attempt_id } });
  });

  it("a session is completed with the server of its own run", async () => {
    const collect = await approvedWithPendingCollect("https://chat.example.com");
    // An SSO start for another server only names the server of the next run.
    setFlowServer("https://other.example.org");
    collect.finish(signedIn);
    await vi.advanceTimersByTimeAsync(10);
    expect(authStore.completeSignIn).toHaveBeenCalledWith("https://chat.example.com", signedIn);
  });

  it("a failed collect is retried while the attempt is still approved", async () => {
    const collect = await approvedWithPendingCollect();
    collect.fail(new HttpError(503, "x", "SERVICE_UNAVAILABLE"));
    await vi.advanceTimersByTimeAsync(10);
    expect(loginFlow().step).toBe("qr");
    await vi.advanceTimersByTimeAsync(1000);
    expect(api.collectAttempt).toHaveBeenCalledTimes(2);
    collect.finish(signedIn);
    await vi.advanceTimersByTimeAsync(10);
    expect(authStore.completeSignIn).toHaveBeenCalledTimes(1);
    expect(loginFlow().step).toBe("form");
  });

  it("LOGIN_ATTEMPT_NOT_READY on collect goes back to polling", async () => {
    const collect = await approvedWithPendingCollect();
    const polls = pendingPolls();
    collect.fail(new HttpError(409, "x", "LOGIN_ATTEMPT_NOT_READY"));
    await vi.advanceTimersByTimeAsync(1100);
    expect(polls.count()).toBe(1);
    expect(loginFlow().step).toBe("qr");
  });

  it("after LOGIN_ATTEMPT_NOT_READY the attempt's expiry counts again (Argon review 6 #7)", async () => {
    api.createQrAttempt.mockResolvedValue(attempt({ expires_at: new Date(Date.now() + 5000).toISOString() }));
    api.pollAttempt.mockResolvedValueOnce(attempt({ state: "approved", qr: null }));
    api.pollAttempt.mockImplementation(() => new Promise(() => undefined));
    api.collectAttempt.mockRejectedValueOnce(new HttpError(409, "x", "LOGIN_ATTEMPT_NOT_READY"));
    await startPhoneSignIn("https://chat.example.com");
    await vi.advanceTimersByTimeAsync(10);
    expect(api.collectAttempt).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(6000);
    expect(loginFlow()).toEqual({ step: "ended", state: "expired", via: "qr" });
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
  });

  it("leaving while the collected session is installed cancels nothing (Argon review 6 #6)", async () => {
    const collect = await approvedWithPendingCollect();
    let installed!: () => void;
    authStore.completeSignIn.mockImplementation(() => new Promise((r) => (installed = () => r({ id: "u1" }))));
    collect.finish({ user: { id: "u1" }, setup_required: false, password_change_required: false });
    await vi.advanceTimersByTimeAsync(10);
    expect(authStore.completeSignIn).toHaveBeenCalledTimes(1);
    // The sign-in screen unmounts while the session is installed.
    await leaveFlow();
    expect(api.cancelAttempt).not.toHaveBeenCalled();
    installed();
    await vi.advanceTimersByTimeAsync(10);
  });

  it("a refused collect gives the attempt up and says why", async () => {
    const collect = await approvedWithPendingCollect();
    collect.fail(new HttpError(403, "x", "TOTP_UNAVAILABLE"));
    await vi.advanceTimersByTimeAsync(10);
    expect(loginFlow().step).toBe("form");
    expect(loginFlowError()).toBeTruthy();
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
  });

  it("an attempt that runs out on the PC's clock is also cancelled", async () => {
    pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt({ expires_at: new Date(Date.now() + 1000).toISOString() }));
    await startPhoneSignIn("https://chat.example.com");
    await vi.advanceTimersByTimeAsync(1100);
    expect(loginFlow()).toEqual({ step: "ended", state: "expired", via: "qr" });
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
  });

  it("phone not at hand waits while the approved attempt is collected", async () => {
    let finish!: (r: unknown) => void;
    api.collectAttempt.mockImplementation(() => new Promise((r) => (finish = r)));
    api.pollAttempt.mockResolvedValueOnce(attempt({ kind: "password", state: "approved", totp_available: true, qr: null }));
    api.pollAttempt.mockImplementation(() => new Promise(() => undefined));
    applyLoginStep({ status: "confirmation_required", attempt: attempt({ kind: "password", totp_available: true }) }, "password");
    await vi.advanceTimersByTimeAsync(10);
    usePhoneNotAtHand();
    expect(loginFlow().step).toBe("confirm");
    finish(signedIn);
    await vi.advanceTimersByTimeAsync(10);
    expect(authStore.completeSignIn).toHaveBeenCalledTimes(1);
  });

  it("an SSO step cancels the phone attempt it replaces", async () => {
    pendingPolls();
    api.createQrAttempt.mockResolvedValue(attempt());
    await startPhoneSignIn("https://chat.example.com");
    const sso = attempt({ attempt_id: "66666666-6666-4666-8666-666666666666", kind: "oidc" });
    applyLoginStep({ status: "confirmation_required", attempt: sso }, "oidc");
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
    expect(loginFlow()).toMatchObject({ step: "confirm", attempt: { attempt_id: sso.attempt_id } });
  });
});

describe("login flow review round (Fable m2, Argon 6 #5-#7, Codex M1)", () => {
  beforeEach(() => {
    authStore.forgetStepPassword.mockClear();
  });

  it("leaving, ending or failing a step forgets the typed password", async () => {
    api.cancelAttempt.mockResolvedValue(undefined);
    api.pollAttempt.mockImplementation(() => new Promise(() => undefined));
    applyLoginStep({ status: "confirmation_required", attempt: attempt({ kind: "password" }) }, "password");
    await leaveFlow();
    expect(authStore.forgetStepPassword).toHaveBeenCalled();
    authStore.forgetStepPassword.mockClear();
    api.pollAttempt.mockResolvedValueOnce(attempt({ kind: "password", state: "denied" }));
    applyLoginStep({ status: "confirmation_required", attempt: attempt({ kind: "password" }) }, "password");
    await vi.waitFor(() => expect(loginFlow().step).toBe("ended"));
    expect(authStore.forgetStepPassword).toHaveBeenCalled();
  });

  it("a new phone sign-in gives the old attempt up without waiting for the server", async () => {
    let releaseCancel!: () => void;
    api.cancelAttempt.mockImplementation(() => new Promise<void>((r) => (releaseCancel = r)));
    api.pollAttempt.mockImplementation(() => new Promise(() => undefined));
    applyLoginStep({ status: "confirmation_required", attempt: attempt({ kind: "password" }) }, "password");
    const second = attempt({ attempt_id: "55555555-5555-4555-8555-555555555555" });
    api.createQrAttempt.mockResolvedValue(second);
    const started = startPhoneSignIn("https://chat.example.com");
    // The QR attempt is asked for before the old cancel has answered.
    await vi.waitFor(() => expect(api.createQrAttempt).toHaveBeenCalled());
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
    await started;
    releaseCancel();
    api.cancelAttempt.mockResolvedValue(undefined);
    expect(loginFlow()).toMatchObject({ step: "qr", attempt: { attempt_id: second.attempt_id } });
  });

  it("another sign-in choice makes a running password step stale", async () => {
    let answer!: (v: unknown) => void;
    authStore.login.mockImplementation(() => new Promise((r) => (answer = r)));
    api.cancelAttempt.mockResolvedValue(undefined);
    const pending = submitPassword("https://chat.example.com", "alice", "pw");
    authStore.beginSignInChoice(); // e.g. the passkey button
    answer({ status: "confirmation_required", attempt: attempt({ kind: "password" }) });
    await pending;
    expect(api.cancelAttempt).toHaveBeenCalledWith(ID);
    expect(authStore.forgetStepPassword).toHaveBeenCalled();
    expect(loginFlow().step).toBe("form");
  });
});
