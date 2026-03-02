import { test, expect } from "@playwright/test";
import { registerAndReachMain, goHome } from "./helpers";

test.describe("Direct Messages", () => {
  test("new DM button opens modal", async ({ page }) => {
    await registerAndReachMain(page);
    await goHome(page);

    await page.getByTestId("new-dm-button").click();
    await expect(page.getByTestId("new-dm-input")).toBeVisible({
      timeout: 5000,
    });
    await expect(page.getByTestId("new-dm-submit")).toBeVisible();
  });

  test("DM creation with friend", async ({ page, browser }) => {
    // Register two users and make them friends
    await registerAndReachMain(page, {
      usernamePrefix: "dm1",
    });

    const context2 = await browser.newContext({ ignoreHTTPSErrors: true });
    const page2 = await context2.newPage();
    const { username: user2 } = await registerAndReachMain(page2, {
      usernamePrefix: "dm2",
    });

    // User1 sends friend request to user2
    await goHome(page);
    await page.getByTestId("add-friend-button").click();
    await page.getByTestId("add-friend-input").fill(user2);
    await page.getByTestId("add-friend-submit").click();
    await expect(page.getByText(/sent successfully/i)).toBeVisible({
      timeout: 10000,
    });

    // User2 accepts the friend request
    await goHome(page2);
    await page2.getByTestId("friends-tab-pending").click();
    // Wait for the pending request to appear and accept it
    const acceptBtn = page2.getByRole("button", { name: "Accept" });
    await expect(acceptBtn).toBeVisible({ timeout: 10000 });
    await acceptBtn.click();

    // User1 creates a DM with user2
    await goHome(page);
    await page.getByTestId("new-dm-button").click();
    await page.getByTestId("new-dm-input").fill(user2);

    // Select the friend from the list and create DM
    const friendItem = page.getByText(user2).first();
    await expect(friendItem).toBeVisible({ timeout: 5000 });
    await friendItem.click();
    await page.getByTestId("new-dm-submit").click();

    // Verify message input is visible in the DM
    await expect(page.getByTestId("message-input")).toBeVisible({
      timeout: 15000,
    });

    await context2.close();
  });
});
