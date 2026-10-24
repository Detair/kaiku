/**
 * Session-services lifecycle, run against the REAL websocket, presence and
 * preferences stores (native mode) with deferred Tauri `listen` and
 * `invoke`-level mocks. The mock-level interleavings live in auth.test.ts;
 * these tests pin the concrete side effects: which Tauri listeners exist,
 * which preferences are applied, and whether the socket state survives.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { listenMock, invokeMock } = vi.hoisted(() => ({
  listenMock: vi.fn(),
  invokeMock: vi.fn(),
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/core", async (importOriginal) => ({
  ...(await importOriginal<typeof import("@tauri-apps/api/core")>()),
  isTauri: () => true,
  invoke: invokeMock,
}));

vi.mock("@/lib/tauri", () => ({
  // auth store
  getCurrentUser: vi.fn(),
  login: vi.fn(),
  register: vi.fn(),
  logout: vi.fn(),
  oidcCompleteLogin: vi.fn(),
  refreshAccessToken: vi.fn(),
  updatePassword: vi.fn(),
  getAccessToken: vi.fn(() => "token"),
  expireNativeSession: vi.fn(),
  getServerUrl: vi.fn(() => "https://server.test"),
  initNativeAuthEvents: vi.fn(),
  // websocket store
  wsConnect: vi.fn(),
  wsDisconnect: vi.fn(),
  wsSubscribe: vi.fn(),
  wsUnsubscribe: vi.fn(),
  wsTyping: vi.fn(),
  wsStopTyping: vi.fn(),
  getBrowserWebSocket: vi.fn(() => null),
  wsStatus: vi.fn(),
  wsSend: vi.fn(),
  wsPing: vi.fn(),
  wsReconnectNow: vi.fn(),
  setBrowserSocketHooks: vi.fn(),
  ensureFreshAccessToken: vi.fn(),
  isPasswordChangePending: vi.fn(),
  // presence / preferences stores
  updateStatus: vi.fn(),
  fetchApi: vi.fn(),
}));

// Cross-store dependencies of the websocket store, stubbed as in websocket.test.ts.
vi.mock("@/stores/messages", () => ({
  addMessage: vi.fn(),
  removeMessage: vi.fn(),
  messagesState: { byChannel: {} },
  setMessagesState: vi.fn(),
  loadInitialMessages: vi.fn().mockResolvedValue(undefined),
  catchUpChannel: vi.fn().mockResolvedValue("caught_up"),
  catchUpAnchor: vi.fn(),
  clearChannelMessages: vi.fn(),
  getChannelMessages: vi.fn(() => []),
}));
vi.mock("@/stores/threads", () => ({
  addThreadReply: vi.fn(),
  removeThreadReply: vi.fn(),
  setThreadReadState: vi.fn(),
  updateThreadInfo: vi.fn(),
  updateParentThreadIndicator: vi.fn(),
  markThreadUnread: vi.fn(),
  clearThreadUnread: vi.fn(),
  threadsState: { activeThreadId: null },
}));
vi.mock("@/stores/call", () => ({
  receiveIncomingCall: vi.fn(),
  callConnected: vi.fn(),
  callEndedExternally: vi.fn(),
  participantJoined: vi.fn(),
  participantLeft: vi.fn(),
  callState: { status: "idle" },
}));
vi.mock("@/stores/friends", () => ({
  loadFriends: vi.fn(),
  loadPendingRequests: vi.fn(),
  handleUserBlocked: vi.fn(),
  handleUserUnblocked: vi.fn(),
  setFriendsState: vi.fn(),
}));
vi.mock("@/stores/channels", () => ({
  getChannel: vi.fn(),
  channelsState: { selectedChannelId: null, channels: [] as unknown[] },
  handleChannelReadEvent: vi.fn(),
  incrementUnreadCount: vi.fn(),
  updateChannelLastMessageId: vi.fn(),
  refreshChannelsForGuild: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@/stores/guilds", () => ({
  guildsState: { activeGuildId: null as string | null },
  getGuildIdForChannel: vi.fn(),
  incrementGuildUnread: vi.fn(),
  DISCOVERY_SENTINEL: "__discovery__",
}));
vi.mock("@/stores/dms", () => ({
  dmsState: { dms: [], selectedDMId: null, isShowingFriends: false },
  handleDMReadEvent: vi.fn(),
  handleDMNameUpdated: vi.fn(),
  updateDMLastMessage: vi.fn(),
  loadDMs: vi.fn().mockResolvedValue(undefined),
  handleDmCreated: vi.fn(),
}));
vi.mock("@/stores/focus", () => ({
  handleActivityChange: vi.fn(),
  deactivateFocusMode: vi.fn(),
}));
vi.mock("@/stores/drafts", () => ({ clearAllDrafts: vi.fn(), cleanupDrafts: vi.fn() }));
vi.mock("@/stores/e2ee", () => ({ resetE2EEState: vi.fn() }));
vi.mock("@/lib/idleDetector", () => ({
  startIdleDetection: vi.fn(),
  stopIdleDetection: vi.fn(),
  setIdleTimeout: vi.fn(),
}));
vi.mock("@/lib/sound", () => ({ playNotification: vi.fn() }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import * as tauri from "@/lib/tauri";
import { addMessage } from "@/stores/messages";
import type { User } from "@/lib/types";
import { authState, initAuth, login, logout, setAuthState } from "../auth";
import { wsState } from "../websocket";
import { preferences } from "../preferences";

interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
}
function deferred<T = void>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

type Listener = (event: { payload: unknown }) => unknown;
interface Registration {
  name: string;
  callback: Listener;
}

/** Tauri listeners currently registered (duplicates count separately). */
let registered: Registration[] = [];
/** Gates for upcoming `listen` calls: each held call registers once released. */
let listenGates: Array<Deferred<void>> = [];

/** Keep the next `count` listen registrations pending until the result runs. */
function holdNextListens(count: number): () => void {
  const gates = Array.from({ length: count }, () => deferred<void>());
  listenGates.push(...gates);
  return () => gates.forEach((g) => g.resolve());
}

function listenerNames(): string[] {
  return registered.map((r) => r.name).sort();
}

const SESSION_LISTENERS = [
  "presence:activity_changed",
  "ws:auth_rejected",
  "ws:connected",
  "ws:connecting",
  "ws:disconnected",
  "ws:reconnecting",
  "ws:server_event",
];

function user(id: string): User {
  return {
    id,
    username: id,
    display_name: id,
    avatar_url: null,
    status: "online",
    email: null,
    mfa_enabled: false,
    created_at: "2025-01-01T00:00:00Z",
  };
}

/** Real macrotask turns, so dynamic imports and IPC-style promises settle. */
async function flush(turns = 8): Promise<void> {
  for (let i = 0; i < turns; i++) {
    await new Promise<void>((r) => setImmediate(r));
  }
}

/** Assert `p` has settled by now (after a flush), without waiting for it. */
async function expectSettled(p: Promise<unknown>, what: string): Promise<void> {
  let done = false;
  p.then(
    () => (done = true),
    () => (done = true),
  );
  await flush();
  expect(done, `${what} is still pending`).toBe(true);
}

function prefsResponse(theme: string, updatedAt: string) {
  return { preferences: { theme }, updated_at: updatedAt };
}

describe("session lifecycle with the real service stores", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    registered = [];
    listenGates = [];
    localStorage.clear();
    listenMock.mockImplementation(async (name: string, callback: Listener) => {
      const gate = listenGates.shift();
      if (gate) await gate.promise;
      const entry = { name, callback };
      registered.push(entry);
      return () => {
        registered = registered.filter((r) => r !== entry);
      };
    });
    invokeMock.mockResolvedValue(null);
    vi.mocked(tauri.initNativeAuthEvents).mockResolvedValue(undefined);
    vi.mocked(tauri.logout).mockResolvedValue(undefined);
    vi.mocked(tauri.wsConnect).mockResolvedValue(undefined);
    vi.mocked(tauri.wsDisconnect).mockResolvedValue(undefined);
    vi.mocked(tauri.ensureFreshAccessToken).mockResolvedValue(true);
    vi.mocked(tauri.isPasswordChangePending).mockResolvedValue(false);
    vi.mocked(tauri.fetchApi).mockResolvedValue(prefsResponse("focused-hybrid", "2026-01-01T00:00:00Z"));
    vi.mocked(tauri.getCurrentUser).mockResolvedValue({ user: user("alice"), error_reason: null });
    vi.mocked(tauri.login).mockResolvedValue({
      user: user("bob"),
      setup_required: false,
      password_change_required: false,
    });
    setAuthState({
      user: null,
      serverUrl: null,
      isLoading: false,
      isInitialized: false,
      error: null,
      setupRequired: false,
      sessionExpired: false,
      restorePending: false,
      passwordChangeRequired: false,
    });
  });

  afterEach(async () => {
    vi.useRealTimers();
    listenGates.forEach((g) => g.resolve());
    listenGates = [];
    await logout();
    await flush();
  });

  it("logout while the WebSocket listeners register leaves no Tauri listener behind", async () => {
    // Restore A: every listener registration (6 WebSocket + 1 presence) is
    // still waiting on IPC when the user signs out.
    const release = holdNextListens(SESSION_LISTENERS.length);
    const restoring = initAuth();
    await flush();
    expect(listenMock).toHaveBeenCalledTimes(SESSION_LISTENERS.length);

    await expectSettled(logout(), "logout");
    expect(authState.user).toBeNull();

    // The registrations complete after the sign-out: none may survive.
    release();
    await expectSettled(restoring, "the interrupted restore");
    await flush();

    expect(listenerNames()).toEqual([]);
  });

  describe("a startup that settles only after the next sign-in", () => {
    beforeEach(() => {
      vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    });

    /** Let every bounded wait in the lifecycle expire, then settle promises. */
    async function expireWaits(): Promise<void> {
      await vi.advanceTimersByTimeAsync(10_000);
      await flush();
    }

    it("leaves exactly the new session's listeners, each handling a server event once, and its logout removes them all", async () => {
      const release = holdNextListens(SESSION_LISTENERS.length);
      const restoring = initAuth();
      await flush();
      expect(listenMock).toHaveBeenCalledTimes(SESSION_LISTENERS.length);
      await expectSettled(logout(), "logout");

      // B signs in while A's registrations are still in flight.
      const signingIn = login("https://server.test", "bob", "pw");
      await expireWaits();
      await expectSettled(signingIn, "B's sign-in");
      expect(listenerNames()).toEqual(SESSION_LISTENERS);
      expect(wsState.status).toBe("connected");

      // A's registrations complete now: they must not add to B's.
      release();
      await expectSettled(restoring, "A's restore");
      await flush();
      expect(listenerNames()).toEqual(SESSION_LISTENERS);

      // One server event is handled exactly once.
      const message = {
        id: "m1",
        channel_id: "c1",
        content: "hi",
        author: { id: "carol", username: "carol" },
        created_at: "2026-01-01T00:00:00Z",
      };
      for (const r of registered.filter((r) => r.name === "ws:server_event")) {
        await r.callback({ payload: { type: "message_new", channel_id: "c1", message } });
      }
      await flush();
      expect(addMessage).toHaveBeenCalledTimes(1);

      // B's sign-out removes everything.
      await expectSettled(logout(), "B's logout");
      expect(listenerNames()).toEqual([]);
    });

    it("keeps the new session's preferences when the old preferences response arrives late", async () => {
      // A's preferences fetch hangs (listeners and connect are fine).
      const oldFetch = deferred<unknown>();
      vi.mocked(tauri.fetchApi).mockImplementationOnce(() => oldFetch.promise as Promise<never>);
      const restoring = initAuth();
      await flush();
      expect(tauri.fetchApi).toHaveBeenCalledTimes(1);
      await expectSettled(logout(), "logout");

      // B signs in; the server has B's preferences.
      vi.mocked(tauri.fetchApi).mockResolvedValue(prefsResponse("solarized-light", "2026-02-01T00:00:00Z"));
      const signingIn = login("https://server.test", "bob", "pw");
      await expireWaits();
      await expectSettled(signingIn, "B's sign-in");
      expect(preferences().theme).toBe("solarized-light");

      // A's response arrives late, and is even "newer": it must be discarded.
      oldFetch.resolve(prefsResponse("solarized-dark", "2026-03-01T00:00:00Z"));
      await expectSettled(restoring, "A's restore");
      await flush();

      expect(preferences().theme).toBe("solarized-light");
      const stored = JSON.parse(localStorage.getItem("vc:preferences") ?? "{}");
      expect(stored.data?.theme).toBe("solarized-light");
    });

    it("a disconnect that settles after the next connect leaves that connection connected", async () => {
      await initAuth();
      await flush();
      expect(wsState.status).toBe("connected");

      // Signing out: the native disconnect hangs on IPC.
      const oldDisconnect = deferred<void>();
      vi.mocked(tauri.wsDisconnect).mockImplementationOnce(() => oldDisconnect.promise);
      const loggingOut = logout();
      await expireWaits();
      await expectSettled(loggingOut, "logout with a hung disconnect");

      const signingIn = login("https://server.test", "bob", "pw");
      await expireWaits();
      await expectSettled(signingIn, "B's sign-in");
      expect(wsState.status).toBe("connected");

      // The old disconnect resumes: B's connection state must survive.
      oldDisconnect.resolve();
      await flush();
      expect(wsState.status).toBe("connected");
      expect(listenerNames()).toEqual(SESSION_LISTENERS);
    });
  });
});
