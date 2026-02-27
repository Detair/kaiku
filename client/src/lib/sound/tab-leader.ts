/**
 * Tab Leadership Coordination
 *
 * Ensures only one browser tab plays notification sounds to prevent duplicates.
 * Uses BroadcastChannel API with localStorage fallback for Safari <15.4.
 */

// ============================================================================
// Constants
// ============================================================================

const CHANNEL_NAME = "canis:tab-leader";
const STORAGE_KEY = "canis:tab-leader";
const HEARTBEAT_INTERVAL = 2000; // 2 seconds
const LEADER_TIMEOUT = 5000; // 5 seconds without heartbeat = dead leader

// ============================================================================
// State
// ============================================================================

let tabId: string;
let isLeader = false;
let broadcastChannel: BroadcastChannel | null = null;
let heartbeatInterval: number | null = null;
let checkLeaderInterval: number | null = null;

// ============================================================================
// Initialization
// ============================================================================

/**
 * Generate a unique tab ID.
 */
function generateTabId(): string {
  return `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;
}

/**
 * Initialize tab leadership system.
 * Should be called once on app startup.
 */
export function initTabLeader(): void {
  tabId = generateTabId();

  // Try BroadcastChannel first
  if ("BroadcastChannel" in window) {
    initBroadcastChannel();
  } else {
    // Fallback to localStorage
    initLocalStorageFallback();
  }

  // Attempt to become leader
  attemptBecomeLeader();

  // Start checking for dead leaders
  checkLeaderInterval = window.setInterval(
    checkLeaderAlive,
    HEARTBEAT_INTERVAL,
  );

  // Clean up on unload
  window.addEventListener("beforeunload", cleanup);
}

/**
 * Check if this tab is the leader.
 */
export function isTabLeader(): boolean {
  return isLeader;
}

/**
 * Clean up on tab close.
 */
export function cleanup(): void {
  if (heartbeatInterval) {
    clearInterval(heartbeatInterval);
    heartbeatInterval = null;
  }
  if (checkLeaderInterval) {
    clearInterval(checkLeaderInterval);
    checkLeaderInterval = null;
  }

  if (isLeader) {
    // Announce resignation
    broadcastMessage({ type: "leader_resigned", tabId });

    // Clear storage
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {
      // Ignore storage errors
    }
  }

  if (broadcastChannel) {
    broadcastChannel.close();
    broadcastChannel = null;
  }
}

// ============================================================================
// BroadcastChannel Implementation
// ============================================================================

interface LeaderMessage {
  type: "heartbeat" | "leader_claim" | "leader_resigned" | "leader_challenge";
  tabId: string;
  timestamp?: number;
}

function initBroadcastChannel(): void {
  broadcastChannel = new BroadcastChannel(CHANNEL_NAME);

  broadcastChannel.onmessage = (event: MessageEvent<LeaderMessage>) => {
    handleMessage(event.data);
  };
}

function broadcastMessage(message: LeaderMessage): void {
  if (broadcastChannel) {
    broadcastChannel.postMessage(message);
  }

  // Also update localStorage for cross-browser compatibility
  try {
    if (message.type === "heartbeat" && isLeader) {
      localStorage.setItem(
        STORAGE_KEY,
        JSON.stringify({ tabId, timestamp: Date.now() }),
      );
    }
  } catch {
    // Ignore storage errors
  }
}

function handleMessage(message: LeaderMessage): void {
  if (message.tabId === tabId) return; // Ignore own messages

  switch (message.type) {
    case "leader_claim":
      // Another tab is claiming leadership
      if (isLeader) {
        // Challenge based on timestamp (earliest wins)
        if (message.timestamp && message.timestamp < Date.now() - 1000) {
          // They were first, give up leadership
          giveUpLeadership();
        }
      }
      break;

    case "leader_resigned":
      // Leader left, attempt to become leader
      attemptBecomeLeader();
      break;

    case "heartbeat":
      // Update our record of the leader
      break;
  }
}

// ============================================================================
// localStorage Fallback
// ============================================================================

function initLocalStorageFallback(): void {
  // Poll localStorage for changes
  window.addEventListener("storage", (event) => {
    if (event.key === STORAGE_KEY) {
      if (!event.newValue && isLeader) {
        // Someone cleared our leadership
        isLeader = false;
        attemptBecomeLeader();
      }
    }
  });
}

// ============================================================================
// Leadership Logic
// ============================================================================

function attemptBecomeLeader(): void {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);

    if (stored) {
      const data = JSON.parse(stored);
      const age = Date.now() - data.timestamp;

      if (age < LEADER_TIMEOUT) {
        // Current leader is still alive
        return;
      }
    }

    // Become leader
    becomeLeader();
  } catch {
    // If localStorage fails, just become leader
    becomeLeader();
  }
}

function becomeLeader(): void {
  isLeader = true;

  // Announce leadership
  broadcastMessage({
    type: "leader_claim",
    tabId,
    timestamp: Date.now(),
  });

  // Start heartbeat
  if (heartbeatInterval) {
    clearInterval(heartbeatInterval);
  }
  heartbeatInterval = window.setInterval(sendHeartbeat, HEARTBEAT_INTERVAL);
  sendHeartbeat(); // Send immediately
}

function giveUpLeadership(): void {
  isLeader = false;
  if (heartbeatInterval) {
    clearInterval(heartbeatInterval);
    heartbeatInterval = null;
  }
}

function sendHeartbeat(): void {
  if (!isLeader) return;

  broadcastMessage({
    type: "heartbeat",
    tabId,
    timestamp: Date.now(),
  });
}

function checkLeaderAlive(): void {
  if (isLeader) return; // We are the leader

  try {
    const stored = localStorage.getItem(STORAGE_KEY);

    if (!stored) {
      // No leader, attempt to become one
      attemptBecomeLeader();
      return;
    }

    const data = JSON.parse(stored);
    const age = Date.now() - data.timestamp;

    if (age > LEADER_TIMEOUT) {
      // Leader is dead, attempt to become one
      attemptBecomeLeader();
    }
  } catch {
    // If checking fails, try to become leader
    attemptBecomeLeader();
  }
}
