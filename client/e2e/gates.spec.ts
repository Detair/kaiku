import { test, expect } from "@playwright/test";
import { registerAndReachMain, uniqueId } from "./helpers";

test.describe("Global gate flow", () => {
  test.describe.configure({ mode: "serial" });

  test("handles setup, onboarding, and acceptance manager with real backend", async ({ page }) => {
    const slug = uniqueId("e2e-required").toLowerCase();

    await registerAndReachMain(page, {
      usernamePrefix: "gate",
      setupServerName: "E2E Gate Server",
    });

    await page.goto("/admin");
    await expect(page.getByRole("heading", { name: "Admin Dashboard" })).toBeVisible({
      timeout: 15000,
    });
    await page.getByRole("button", { name: "Platform Pages" }).click();
    await page.getByTestId("new-platform-page").click();

    await page.getByTestId("page-editor-title").fill("E2E Required Terms");
    await page.getByTestId("page-editor-slug").fill(slug);
    await page
      .getByTestId("page-editor-content")
      .fill("Required terms for real gate verification.");
    await page.getByTestId("page-editor-requires-acceptance").check();
    await page.getByTestId("page-editor-save").click();
    await expect(page.getByText("E2E Required Terms")).toBeVisible({ timeout: 15000 });

    await page.goto("/");

    await expect(page.getByText("E2E Required Terms")).toBeVisible({ timeout: 15000 });
    await expect(page.getByTestId("page-acceptance-remind-later")).toHaveCount(0);

    // Scroll content to enable the accept button, then click
    const acceptButton = page.getByTestId("page-acceptance-accept");
    const modalContent = page.getByTestId("page-acceptance-content");
    await modalContent.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
      el.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    await expect(acceptButton).toBeEnabled({ timeout: 10000 });
    await acceptButton.click();

    await expect(page.getByText("E2E Required Terms")).toBeHidden({ timeout: 15000 });
    await expect(page.getByTestId("user-settings-button")).toBeVisible({
      timeout: 10000,
    });
  });
});
