// A11Y-18 / UX4b K6: on a page route the window title names the page.
import { describe, it, expect, vi } from "vitest";
import { render, waitFor } from "@solidjs/testing-library";

vi.mock("@solidjs/router", () => ({
  useParams: () => ({ slug: "rules", guildId: "g1" }),
  useNavigate: () => vi.fn(),
}));
vi.mock("@/lib/tauri", () => ({
  getGuildPage: vi.fn(async () => ({ id: "p1", title: "House rules", content: "Be kind.", guild_id: "g1" })),
  getPlatformPage: vi.fn(),
}));
vi.mock("@/components/pages", () => ({ PageView: () => <div /> }));

import PageViewRoute from "../PageViewRoute";

describe("PageViewRoute title", () => {
  it("names the page and goes back to the app name when left", async () => {
    document.title = "#general – Familie – Kaiku";
    const view = render(() => <PageViewRoute />);
    await waitFor(() => expect(document.title).toBe("House rules – Kaiku"));
    view.unmount();
    expect(document.title).toBe("Kaiku");
  });
});
