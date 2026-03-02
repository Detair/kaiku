import { test, expect } from "@playwright/test";
import {
  registerAndReachMain,
  uniqueId,
  createTextChannel,
  selectChannel,
  sendMessage,
  ensureGuildSelected,
} from "./helpers";

test.describe("Messaging", () => {
  test("send and display a message", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const msg = `hello-${uniqueId("msg")}`;
    await sendMessage(page, msg);
    await expect(page.getByText(msg)).toBeVisible();
  });

  test("empty message is not sent", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const input = page.getByTestId("message-input");
    await expect(input).toBeVisible({ timeout: 10000 });

    const beforeCount = await page.getByTestId("message-item").count();
    await input.press("Enter");

    // Verify message count stays stable (no new messages appear)
    await expect
      .poll(() => page.getByTestId("message-item").count(), { timeout: 3000 })
      .toBe(beforeCount);
  });

  test("markdown bold renders correctly", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    await sendMessage(page, "**bold text**");
    await expect(
      page.locator("strong").filter({ hasText: "bold text" })
    ).toBeVisible({ timeout: 15000 });
  });

  test("code block renders correctly", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const input = page.getByTestId("message-input");
    await expect(input).toBeVisible({ timeout: 10000 });
    await input.fill("```js\nconsole.log('hello')\n```");
    await input.press("Enter");

    await expect(
      page.locator("code, pre").filter({ hasText: "console.log" })
    ).toBeVisible({ timeout: 15000 });
  });

  test("multi-line message with Shift+Enter", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const input = page.getByTestId("message-input");
    await expect(input).toBeVisible({ timeout: 10000 });

    const line1 = `line1-${uniqueId("ml")}`;
    const line2 = `line2-${uniqueId("ml")}`;
    await input.fill(line1);
    await input.press("Shift+Enter");
    await input.pressSequentially(line2);
    await input.press("Enter");

    await expect(page.getByText(line1)).toBeVisible({ timeout: 15000 });
    await expect(page.getByText(line2)).toBeVisible({ timeout: 15000 });
  });

  test("edit a sent message", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const original = `edit-orig-${uniqueId("msg")}`;
    await sendMessage(page, original);

    // Hover message, click more actions, click edit
    const messageItem = page.getByTestId("message-item").filter({ hasText: original });
    await messageItem.hover();
    await page.getByTestId("message-action-more").click();
    await page.getByText(/edit/i).first().click();

    // Update message content
    const edited = `edit-updated-${uniqueId("msg")}`;
    const editInput = messageItem.locator("textarea, input").first();
    await expect(editInput).toBeVisible({ timeout: 5000 });
    await editInput.fill(edited);
    await editInput.press("Enter");

    await expect(page.getByText(edited)).toBeVisible({ timeout: 15000 });
  });

  test("delete a sent message", async ({ page }) => {
    await registerAndReachMain(page);
    await ensureGuildSelected(page);
    const channelName = await createTextChannel(page);
    await selectChannel(page, channelName);

    const msg = `delete-me-${uniqueId("msg")}`;
    await sendMessage(page, msg);

    // Hover message, click more actions, click delete
    const messageItem = page.getByTestId("message-item").filter({ hasText: msg });
    await messageItem.hover();
    await page.getByTestId("message-action-more").click();
    await page.getByText(/delete/i).first().click();

    // Confirm deletion if a confirmation dialog appears
    const confirmBtn = page.getByRole("button", { name: /confirm|delete/i });
    if (await confirmBtn.isVisible()) {
      await confirmBtn.click();
    }

    await expect(page.getByText(msg)).toBeHidden({ timeout: 15000 });
  });
});
