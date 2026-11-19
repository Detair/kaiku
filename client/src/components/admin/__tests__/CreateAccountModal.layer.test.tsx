import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@solidjs/testing-library";
vi.mock("@/lib/tauri", () => ({ getServerUrl: () => "https://kaiku.example" }));
vi.mock("@/stores/admin", () => ({ adminState: { isElevated: true }, createUser: vi.fn() }));
vi.mock("@/stores/guilds", () => ({ guildsState: { guilds: [] } }));
vi.mock("@/components/ui/Toast", () => ({ showToast: vi.fn() }));

import CreateAccountModal from "../CreateAccountModal";
import { createModalLayer, MODAL_LAYER } from "@/lib/a11y/modalLayer";

afterEach(cleanup);

const zOf = (el: HTMLElement): number => {
  for (let n: HTMLElement | null = el; n; n = n.parentElement) {
    if (n.style.zIndex) return Number(n.style.zIndex);
  }
  return 0;
};

describe("CreateAccountModal layer", () => {
  it("opens in front of a window that is already open (it takes the keyboard)", async () => {
    // For example the admin's server settings, which sit on the dialog layer.
    render(() => {
      const layer = createModalLayer(MODAL_LAYER.dialog);
      return <div data-testid="earlier" style={{ "z-index": layer }} />;
    });
    render(() => <CreateAccountModal onClose={() => {}} />);
    await Promise.resolve();
    const earlier = Number(screen.getByTestId("earlier").style.zIndex);
    expect(zOf(screen.getByRole("dialog"))).toBeGreaterThan(earlier);
  });
});
