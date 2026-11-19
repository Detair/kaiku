import { describe, it, expect, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import Avatar from "../Avatar";

afterEach(() => cleanup());

// Tailwind 700 shades; white initials must stay >= 4.5:1 on all of them.
const SHADES: Record<string, string> = {
  red: "#b91c1c", orange: "#c2410c", amber: "#b45309", yellow: "#a16207",
  lime: "#4d7c0f", green: "#15803d", emerald: "#047857", teal: "#0f766e",
  cyan: "#0e7490", sky: "#0369a1", blue: "#1d4ed8", indigo: "#4338ca",
  violet: "#6d28d9", purple: "#7e22ce", fuchsia: "#a21caf", pink: "#be185d",
};

function lum(hex: string): number {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);
  const l = c.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * l[0] + 0.7152 * l[1] + 0.0722 * l[2];
}

describe("Avatar initials (A11Y-4)", () => {
  it("is never smaller than 10 px", () => {
    const { container } = render(() => <Avatar alt="Ada Lovelace" size="xs" />);
    expect(container.innerHTML).toContain("text-[10px]");
    expect(container.innerHTML).not.toContain("text-[8px]");
  });

  it("uses colours that keep white initials readable", () => {
    const names = ["Ada", "Bob", "Chen", "Dora", "Eli", "Fay", "Gus", "Hana", "Ivo", "Jo", "Kai", "Lea", "Mo", "Nia", "Otto", "Pia", "Quentin", "Rosa", "Sam", "Tess"];
    for (const n of names) {
      const { container, unmount } = render(() => <Avatar alt={n} />);
      const m = container.innerHTML.match(/bg-([a-z]+)-(\d+)/);
      expect(m, n).not.toBeNull();
      expect(m![2]).toBe("700");
      const ratio = 1.05 / (lum(SHADES[m![1]]) + 0.05);
      expect(ratio, `${n}: white on ${m![0]}`).toBeGreaterThanOrEqual(4.5);
      unmount();
    }
  });
});
