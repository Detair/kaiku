import { test, expect } from "@playwright/test";
import { registerAndReachMain, uniqueId } from "./helpers";

test.describe("Wiki/Pages System", () => {
  test("create page via admin panel", async ({ page }) => {
    await registerAndReachMain(page, {
      usernamePrefix: "pages",
      setupServerName: "E2E Pages Server",
    });

    await page.goto("/admin");
    await expect(
      page.getByRole("heading", { name: "Admin Dashboard" }),
    ).toBeVisible({ timeout: 15000 });

    await page.getByRole("button", { name: "Platform Pages" }).click();
    await page.getByTestId("new-platform-page").click();

    const title = `TestPage-${uniqueId("pg")}`;
    const slug = title.toLowerCase().replace(/[^a-z0-9-]/g, "-");

    await page.getByTestId("page-editor-title").fill(title);
    await page.getByTestId("page-editor-slug").fill(slug);
    await page.getByTestId("page-editor-content").fill("Test page content for E2E.");
    await page.getByTestId("page-editor-save").click();

    await expect(page.getByText(title)).toBeVisible({ timeout: 15000 });
  });

  test("acceptance flow blocks and resolves", async ({ page }) => {
    const slug = uniqueId("e2e-terms").toLowerCase();

    await registerAndReachMain(page, {
      usernamePrefix: "terms",
      setupServerName: "E2E Terms Server",
    });

    // Create a required page
    await page.goto("/admin");
    await expect(
      page.getByRole("heading", { name: "Admin Dashboard" }),
    ).toBeVisible({ timeout: 15000 });

    await page.getByRole("button", { name: "Platform Pages" }).click();
    await page.getByTestId("new-platform-page").click();

    await page.getByTestId("page-editor-title").fill("E2E Terms");
    await page.getByTestId("page-editor-slug").fill(slug);
    await page
      .getByTestId("page-editor-content")
      .fill("Required terms content.");
    await page.getByTestId("page-editor-requires-acceptance").check();
    await page.getByTestId("page-editor-save").click();
    await expect(page.getByText("E2E Terms")).toBeVisible({ timeout: 15000 });

    // Navigate to app — acceptance modal should appear
    await page.goto("/");
    await expect(page.getByText("E2E Terms")).toBeVisible({ timeout: 15000 });

    // Scroll content to enable the accept button, then click
    const acceptButton = page.getByTestId("page-acceptance-accept");
    const modalContent = page.getByTestId("page-acceptance-content");
    await modalContent.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
      el.dispatchEvent(new Event("scroll", { bubbles: true }));
    });
    await expect(acceptButton).toBeEnabled({ timeout: 10000 });
    await acceptButton.click();

    await expect(page.getByText("E2E Terms")).toBeHidden({ timeout: 15000 });
    await expect(page.getByTestId("user-settings-button")).toBeVisible({
      timeout: 10000,
    });
  });
});
