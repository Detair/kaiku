import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  uniqueId,
  ensureGuildSelected,
  createTextChannel,
  selectChannel,
  sendMessage,
} from "./helpers";

test.describe("Message Reactions", () => {
  test("add quick reaction to message", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const msg = `react-${uniqueId("rxn")}`;
    await sendMessage(page, msg);

    // Hover to show action bar, click the first quick reaction button
    const messageItem = page.getByTestId("message-item").filter({ hasText: msg });
    await messageItem.hover();

    const actionBar = messageItem.getByTestId("message-action-bar");
    await expect(actionBar).toBeVisible({ timeout: 5000 });
    await actionBar.getByRole("button").first().click();

    // Verify reaction bar appears on the message
    await expect(page.getByTestId("reaction-bar")).toBeVisible({
      timeout: 10000,
    });
  });

  test("emoji picker opens from message actions", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const msg = `emoji-${uniqueId("rxn")}`;
    await sendMessage(page, msg);

    // Hover to show action bar, click emoji picker button
    const messageItem = page.getByTestId("message-item").filter({ hasText: msg });
    await messageItem.hover();
    await page.getByTestId("message-action-react").click();

    await expect(page.getByTestId("emoji-picker")).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByTestId("emoji-search")).toBeVisible();
  });
});
