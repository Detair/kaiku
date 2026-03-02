import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  uniqueId,
  ensureGuildSelected,
  createTextChannel,
  selectChannel,
  sendMessage,
} from "./helpers";

test.describe("Thread Conversations", () => {
  test("open thread from message action", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const msg = `thread-parent-${uniqueId("thr")}`;
    await sendMessage(page, msg);

    // Hover message to show action bar, click thread button
    const messageItem = page.getByTestId("message-item").filter({ hasText: msg });
    await messageItem.hover();
    await page.getByTestId("message-action-thread").click();

    await expect(page.getByTestId("thread-sidebar")).toBeVisible({
      timeout: 10000,
    });
  });

  test("send a reply in thread", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const parentMsg = `thread-parent-${uniqueId("thr")}`;
    await sendMessage(page, parentMsg);

    // Open thread
    const messageItem = page.getByTestId("message-item").filter({ hasText: parentMsg });
    await messageItem.hover();
    await page.getByTestId("message-action-thread").click();
    await expect(page.getByTestId("thread-sidebar")).toBeVisible({
      timeout: 10000,
    });

    // Send reply in thread
    const replyText = `thread-reply-${uniqueId("rep")}`;
    const replyInput = page.getByTestId("thread-reply-input");
    await expect(replyInput).toBeVisible({ timeout: 5000 });
    await replyInput.fill(replyText);
    await replyInput.press("Enter");

    // Verify reply appears in thread sidebar
    await expect(
      page.getByTestId("thread-sidebar").getByText(replyText),
    ).toBeVisible({ timeout: 15000 });
  });
});
