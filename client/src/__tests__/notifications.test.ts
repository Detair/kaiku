import { describe, it, expect, vi, beforeEach } from "vitest";

// We only test the pure formatting function — no mocking needed
import { formatNotificationContent } from "@/lib/notifications";
import type { SoundEvent } from "@/lib/sound/types";
import type { NotificationContext } from "@/lib/notifications";

function makeEvent(overrides: Partial<SoundEvent> = {}): SoundEvent {
  return {
    type: "message_dm",
    channelId: "ch1",
    isDm: true,
    ...overrides,
  };
}

function makeCtx(overrides: Partial<NotificationContext> = {}): NotificationContext {
  return {
    username: "Alice",
    content: "Hey there!",
    guildName: null,
    channelName: null,
    ...overrides,
  };
}

describe("formatNotificationContent", () => {
  it("returns generic body when show_content is false", () => {
    const result = formatNotificationContent(
      makeEvent(),
      makeCtx(),
      false,
    );
    expect(result.title).toBe("Alice");
    expect(result.body).toBe("New message");
  });

  it("returns message preview for DM when show_content is true", () => {
    const result = formatNotificationContent(
      makeEvent(),
      makeCtx(),
      true,
    );
    expect(result.title).toBe("Alice");
    expect(result.body).toBe("Hey there!");
  });

  it("returns mention format for channel mentions", () => {
    const result = formatNotificationContent(
      makeEvent({ type: "message_mention", isDm: false, mentionType: "direct" }),
      makeCtx({ username: "Bob", content: "Check this out", guildName: "Dev Team", channelName: "general" }),
      true,
    );
    expect(result.title).toBe("#general in Dev Team");
    expect(result.body).toBe("@Bob: Check this out");
  });

  it("truncates long message content", () => {
    const longContent = "a".repeat(200);
    const result = formatNotificationContent(
      makeEvent(),
      makeCtx({ content: longContent }),
      true,
    );
    expect(result.body.length).toBeLessThanOrEqual(103); // 100 + "..."
  });

  it("returns generic body for encrypted messages (null content)", () => {
    const result = formatNotificationContent(
      makeEvent(),
      makeCtx({ content: null }),
      true,
    );
    expect(result.body).toBe("New message");
  });

  it("formats thread reply notifications", () => {
    const result = formatNotificationContent(
      makeEvent({ type: "message_thread", isDm: false }),
      makeCtx({ username: "Carol", content: "I agree", guildName: "Dev Team", channelName: "general" }),
      true,
    );
    expect(result.title).toBe("Thread reply in #general");
    expect(result.body).toBe("Carol: I agree");
  });

  it("formats incoming call notifications", () => {
    const result = formatNotificationContent(
      makeEvent({ type: "call_incoming", isDm: true }),
      makeCtx({ username: "Dave", content: null }),
      true,
    );
    expect(result.title).toBe("Incoming call");
    expect(result.body).toBe("Dave is calling you");
  });

  it("shows call notification even with show_content false", () => {
    const result = formatNotificationContent(
      makeEvent({ type: "call_incoming", isDm: true }),
      makeCtx({ username: "Dave", content: null }),
      false,
    );
    expect(result.title).toBe("Incoming call");
    expect(result.body).toBe("Dave is calling you");
  });

  it("formats channel message with guild context", () => {
    const result = formatNotificationContent(
      makeEvent({ type: "message_channel", isDm: false }),
      makeCtx({ username: "Eve", content: "Hello world", guildName: "Gaming", channelName: "lobby" }),
      true,
    );
    expect(result.title).toBe("#lobby in Gaming");
    expect(result.body).toBe("Eve: Hello world");
  });

  it("falls back to username when guild/channel missing for mention", () => {
    const result = formatNotificationContent(
      makeEvent({ type: "message_mention", isDm: false, mentionType: "direct" }),
      makeCtx({ username: "Frank", content: "Hey", guildName: null, channelName: null }),
      true,
    );
    expect(result.title).toBe("Frank");
  });
});
