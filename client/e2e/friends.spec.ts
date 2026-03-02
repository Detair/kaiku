import { test, expect } from "@playwright/test";
import { registerAndReachMain, goHome } from "./helpers";

test.describe("Friends & DMs", () => {
  test("friends list tabs visible at home", async ({ page }) => {
    await registerAndReachMain(page);
    await goHome(page);

    await expect(page.getByTestId("friends-tab-online")).toBeVisible({
      timeout: 10000,
    });
    await expect(page.getByTestId("friends-tab-all")).toBeVisible();
    await expect(page.getByTestId("friends-tab-pending")).toBeVisible();
    await expect(page.getByTestId("friends-tab-blocked")).toBeVisible();
  });

  test("add friend button opens modal", async ({ page }) => {
    await registerAndReachMain(page);
    await goHome(page);

    await page.getByTestId("add-friend-button").click();
    await expect(page.getByTestId("add-friend-input")).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByTestId("add-friend-submit")).toBeVisible();
  });

  test("send friend request shows feedback", async ({ page, browser }) => {
    // Register two users
    await registerAndReachMain(page, {
      usernamePrefix: "friend1",
    });

    const context2 = await browser.newContext({ ignoreHTTPSErrors: true });
    const page2 = await context2.newPage();
    const { username: user2 } = await registerAndReachMain(page2, {
      usernamePrefix: "friend2",
    });

    // User1 sends friend request to user2
    await goHome(page);
    await page.getByTestId("add-friend-button").click();
    await page.getByTestId("add-friend-input").fill(user2);
    await page.getByTestId("add-friend-submit").click();

    // Should show success message
    await expect(page.getByText(/sent successfully/i)).toBeVisible({
      timeout: 10000,
    });

    await context2.close();
  });

  test("DM list visible at home", async ({ page }) => {
    await registerAndReachMain(page);
    await goHome(page);

    // DM section should be visible (even if empty)
    await expect(page.getByText("Direct Messages")).toBeVisible({
      timeout: 10000,
    });
  });
});
