import { test, expect } from "@playwright/test";
import { registerAndReachMain } from "./helpers";

function frameAsText(payload: string | Buffer): string {
  return typeof payload === "string" ? payload : payload.toString("utf-8");
}

function isSetStatusFrame(payload: string, status: "busy" | "online"): boolean {
  try {
    const frame = JSON.parse(payload) as { type?: string; status?: string };
    return frame.type === "set_status" && frame.status === status;
  } catch (error) {
    if (payload.startsWith("{") || payload.startsWith("[")) {
      console.warn(
        `[isSetStatusFrame] Failed to parse JSON-like payload: ${payload.slice(0, 100)}`
      );
    }
    return false;
  }
}

test.describe("Status presence", () => {
  test("updates presence by sending real websocket set_status events", async ({ page }) => {
    const sentFrames: string[] = [];

    page.on("websocket", (ws) => {
      ws.on("framesent", (event) => {
        sentFrames.push(frameAsText(event.payload));
      });
    });

    await registerAndReachMain(page, {
      usernamePrefix: "presence",
      setupServerName: "E2E Presence Server",
    });

    const statusButton = page.getByTestId("change-status-button");
    await expect(statusButton).toBeVisible({ timeout: 15000 });

    await statusButton.click();
    const statusPicker = page.getByTestId("status-picker");
    await expect(statusPicker).toBeVisible({ timeout: 10000 });
    await statusPicker.getByTestId("status-option-dnd").click();

    await expect
      .poll(
        () =>
          sentFrames.some((payload) => isSetStatusFrame(payload, "busy")),
        { timeout: 10000 },
      )
      .toBe(true);

    await statusButton.click();
    await expect(statusPicker).toBeVisible({ timeout: 10000 });
    await statusPicker.getByTestId("status-option-online").click();

    await expect
      .poll(
        () =>
          sentFrames.some((payload) => isSetStatusFrame(payload, "online")),
        { timeout: 10000 },
      )
      .toBe(true);
  });
});
