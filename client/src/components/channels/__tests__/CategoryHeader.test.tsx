// NAV-20: a category header is a button that announces open or closed.
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import CategoryHeader from "../CategoryHeader";

afterEach(cleanup);

describe("CategoryHeader", () => {
  it("toggles by button and tells whether it is open", () => {
    const onToggle = vi.fn();
    render(() => (
      <CategoryHeader
        id="c1"
        name="Family"
        collapsed={true}
        hasUnread={true}
        isSubcategory={false}
        onToggle={onToggle}
      />
    ));
    const toggle = screen.getByTestId("category-toggle");
    expect(toggle.tagName).toBe("BUTTON");
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(toggle.textContent).toContain("Unread messages");
    fireEvent.click(toggle);
    expect(onToggle).toHaveBeenCalledTimes(1);
  });

  it("does not render action buttons nobody may use", () => {
    render(() => (
      <CategoryHeader
        id="c1"
        name="Family"
        collapsed={false}
        hasUnread={false}
        isSubcategory={false}
        onToggle={() => {}}
      />
    ));
    expect(screen.queryByLabelText("Create Channel")).toBeNull();
  });
});
