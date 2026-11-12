/**
 * Hit areas of small icon buttons (A11Y-12, NAV-19, UX4b fix round W2).
 *
 * jsdom has no layout, so this test does the geometry itself: it takes the
 * real class strings from the components, lets the real UnoCSS config turn
 * them into CSS (desktop, and touch = `@media (hover: none)`), and adds the
 * `tap-target` hit area from global.css. Then it lays each button group out
 * as a flex row and checks that no two hit areas overlap and that no hit
 * area sticks out of its row into the neighbouring row.
 *
 * Assumptions (written down because the browser is not here):
 * - Buttons sit side by side in a flex row, in source order, separated by the
 *   container's `gap-*` plus their own `ml-*`.
 * - A button's box is max(width, min-width) and max(height, min-height);
 *   without an explicit width it is padding + icon. Icon sizes are given per
 *   group (Tailwind `w-3.5` = 14 px, `w-4` = 16 px).
 * - `tap-target` grows the hit area around the centre to 24 px, 44 px on
 *   touch (the values are read from global.css), and is the only thing that
 *   makes a hit area larger than the box.
 * - Touch means `hover: none` and `pointer: coarse` both apply (phones,
 *   tablets, Chrome touch emulation).
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { createGenerator } from "unocss";
import { describe, expect, it } from "vitest";
import config from "../../../uno.config";

const src = resolve(__dirname, "..", "..");
const read = (file: string) => readFileSync(resolve(src, file), "utf8");
const globalCss = read("styles/global.css");
const unoPromise = createGenerator({ ...config, safelist: [] });

const px = (v: string | undefined): number | undefined => {
  if (!v) return undefined;
  const m = /^(-?[\d.]+)(rem|px)$/.exec(v.trim());
  if (!m) return undefined;
  return Number(m[1]) * (m[2] === "rem" ? 16 : 1);
};

/** Hit-area sizes of `.tap-target::after` from global.css: [desktop, touch]. */
function tapTargetSizes(): [number, number] {
  const sizes = [...globalCss.matchAll(/\.tap-target::after\s*\{[^}]*width:\s*max\(100%,\s*(\d+)px\)/g)].map(
    (m) => Number(m[1]),
  );
  expect(sizes.length).toBe(2);
  return [sizes[0], sizes[1]];
}

/** CSS declarations UnoCSS applies to an element with these classes. */
async function declsFor(classes: string, touch: boolean): Promise<Record<string, string>> {
  const uno = await unoPromise;
  const { css } = await uno.generate(classes, { preflights: false });
  const body = css.replace(/\/\*[\s\S]*?\*\//g, "");
  const out: Record<string, string> = {};
  const media = /@media\s*\(hover:\s*none\)\s*\{((?:[^{}]*\{[^{}]*\})*)\s*\}/g;
  const base = body.replace(media, "");
  const apply = (chunk: string) => {
    for (const m of chunk.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      const sel = m[1].trim();
      // Plain class selectors only: no :hover, no group-*, no descendants.
      if (!/^\.[^\s:>+~]+(?:\\:[^\s:>+~]+)*$/.test(sel.replace(/\\[:/[\].]/g, "x"))) continue;
      for (const d of m[2].split(";")) {
        const i = d.indexOf(":");
        if (i > 0) out[d.slice(0, i).trim()] = d.slice(i + 1).trim();
      }
    }
  };
  apply(base);
  if (touch) for (const m of body.matchAll(media)) apply(m[1]);
  return out;
}

interface Box {
  w: number;
  h: number;
  hitW: number;
  hitH: number;
  ml: number;
}

async function boxOf(classes: string, icon: number, touch: boolean): Promise<Box> {
  const d = await declsFor(classes, touch);
  const pad = px(d["padding"]) ?? 0;
  const w = Math.max(px(d["width"]) ?? 2 * pad + icon, px(d["min-width"]) ?? 0);
  const h = Math.max(px(d["height"]) ?? 2 * pad + icon, px(d["min-height"]) ?? 0);
  const [desk, tch] = tapTargetSizes();
  const grow = /\btap-target\b/.test(classes) ? (touch ? tch : desk) : 0;
  return { w, h, hitW: Math.max(w, grow), hitH: Math.max(h, grow), ml: px(d["margin-left"]) ?? 0 };
}

/** Class strings of the buttons in `file` that carry one of the markers, in order. */
function buttonClasses(file: string, from: string, count: number): string[] {
  const text = read(file);
  const start = text.indexOf(from);
  expect(start, `${from} in ${file}`).toBeGreaterThan(-1);
  const found: string[] = [];
  const re = /<button\b[\s\S]*?\bclass="([^"]*)"/g;
  re.lastIndex = start;
  let m: RegExpExecArray | null;
  while (found.length < count && (m = re.exec(text))) found.push(m[1]);
  expect(found.length).toBe(count);
  return found;
}

/** Gap of the flex container whose class string contains `needle`. */
async function gapOf(file: string, needle: string): Promise<number> {
  const m = new RegExp(`class="([^"]*${needle.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&")}[^"]*)"`).exec(read(file));
  expect(m, `${needle} in ${file}`).not.toBeNull();
  const d = await declsFor(m![1], false);
  return px(d["gap"]) ?? 0;
}

/** Lay boxes out left to right and return [start, end] of each hit area. */
function layout(boxes: Box[], gap: number): [number, number][] {
  let x = 0;
  return boxes.map((b, i) => {
    if (i > 0) x += gap + b.ml;
    const centre = x + b.w / 2;
    x += b.w;
    return [centre - b.hitW / 2, centre + b.hitW / 2];
  });
}

interface Group {
  name: string;
  file: string;
  /** Text right before the first button of the group. */
  from: string;
  count: number;
  container: string;
  icons: number[];
  /** Class string of the row the group sits in, when it must contain the hit areas. */
  row?: string;
}

const GROUPS: Group[] = [
  {
    name: "channel row: favourite star and channel settings",
    file: "components/channels/ChannelItem.tsx",
    from: "Favorite star",
    count: 2,
    container: "absolute right-1 top-1/2",
    icons: [14, 14],
    row: 'data-testid="channel-item"',
  },
  {
    name: "message actions",
    file: "components/messages/MessageActions.tsx",
    from: "Quick emoji reactions",
    count: 5,
    container: "", // ACTION_BAR_BASE, see below
    icons: [16, 16, 16, 16, 16],
  },
  {
    name: "channel list header: microphone test and new channel",
    file: "components/channels/ChannelList.tsx",
    from: "Visible on hover, keyboard focus and touch screens (NAV-19)",
    count: 2,
    container: "flex items-center gap-0.5 opacity-0",
    icons: [16, 16],
  },
  {
    name: "user panel: admin, settings, sign out",
    file: "components/layout/UserPanel.tsx",
    from: "Action buttons: compact group",
    count: 3,
    container: "flex items-center gap-0.5 ml-auto",
    icons: [16, 16, 16],
  },
  {
    name: "category header: new channel and settings",
    file: "components/channels/CategoryHeader.tsx",
    from: "Actions: on hover, keyboard focus and touch screens",
    count: 2,
    container: "flex items-center gap-1 pr-1",
    icons: [14, 14],
  },
  {
    name: "group name: save and cancel",
    file: "components/home/DMConversation.tsx",
    from: "if (e.key === \"Enter\") saveEditedName();",
    count: 2,
    container: "", // siblings of the name field, see below
    icons: [16, 16],
  },
];

async function containerGap(g: Group): Promise<number> {
  if (g.file.endsWith("MessageActions.tsx")) {
    const base = /ACTION_BAR_BASE\s*=\s*\n?\s*"([^"]*)"/.exec(read(g.file));
    expect(base).not.toBeNull();
    return px((await declsFor(base![1], false))["gap"]) ?? 0;
  }
  if (g.file.endsWith("DMConversation.tsx")) {
    // The edit row: `<div class="flex items-center gap-…">` around field and buttons.
    const text = read(g.file);
    const at = text.indexOf(g.from);
    const before = text.slice(0, at);
    const m = [...before.matchAll(/<div class="([^"]*flex[^"]*gap-[^"]*)"/g)].pop();
    expect(m).toBeTruthy();
    return px((await declsFor(m![1], false))["gap"]) ?? 0;
  }
  return gapOf(g.file, g.container);
}

describe("hit areas of neighbouring buttons never overlap (A11Y-12, NAV-19)", () => {
  for (const touch of [false, true]) {
    for (const g of GROUPS) {
      it(`${touch ? "touch" : "desktop"}: ${g.name}`, async () => {
        const classes = buttonClasses(g.file, g.from, g.count);
        const boxes = await Promise.all(classes.map((c, i) => boxOf(c, g.icons[i], touch)));
        const gap = await containerGap(g);
        const spans = layout(boxes, gap);
        for (let i = 1; i < spans.length; i++) {
          expect(
            spans[i][0],
            `${g.name}: button ${i + 1} starts inside button ${i}'s hit area`,
          ).toBeGreaterThanOrEqual(spans[i - 1][1] - 0.01);
        }
        // At least 24 px to hit, 44 px on touch (the box itself or the hit area).
        for (const b of boxes) {
          expect(Math.min(b.hitW, b.hitH)).toBeGreaterThanOrEqual(touch ? 44 : 24);
        }
        if (g.row) {
          const row = new RegExp(`${g.row}[\\s\\S]*?class="([^"]*)"`).exec(read(g.file));
          expect(row).not.toBeNull();
          const r = await declsFor(row![1], touch);
          const rowH = Math.max(
            px(r["min-height"]) ?? 0,
            2 * (px(r["padding-top"]) ?? px(r["padding-block"]) ?? 6) + 20, // text-sm line
          );
          for (const b of boxes) {
            expect(b.hitH, `${g.name}: hit area taller than its row`).toBeLessThanOrEqual(rowH);
          }
        }
      });
    }
  }
});

describe("tap-target on lone buttons stays clear of its neighbours", () => {
  it("rule: 24 px, 44 px on touch, around the centre, position utilities still win", () => {
    expect(tapTargetSizes()).toEqual([24, 44]);
    expect(globalCss).toMatch(/\.tap-target::after\s*\{[^}]*translate\(-50%,\s*-50%\)/);
    expect(globalCss).toMatch(/:where\(\.tap-target\)\s*\{\s*position:\s*relative;/);
  });

  it("events dialog: close button next to “New event” keeps its hit area out of it", async () => {
    // Header: [New event] gap-2 [X]. The X grows (44 - box) / 2 to the left.
    const file = "components/guilds/EventsModal.tsx";
    const [x] = buttonClasses(file, "</Show>\n          <button", 1);
    const box = await boxOf(x, 20, true);
    const gap = await gapOf(file, 'flex items-center gap-2"');
    expect((box.hitW - box.w) / 2).toBeLessThanOrEqual(gap);
  });

  it("phone header: menu button stays inside the 44 px bar", async () => {
    const file = "components/layout/MobileHeader.tsx";
    const [menu] = buttonClasses(file, "<header", 1);
    expect(menu).toContain("tap-target");
    const box = await boxOf(menu, 20, true);
    expect(box.hitH).toBeLessThanOrEqual(44);
  });

  it("no lone tap-target sits in a group of buttons", () => {
    // Groups use tap-dense (a real box); tap-target only on buttons alone in their bar.
    for (const g of GROUPS) {
      for (const c of buttonClasses(g.file, g.from, g.count)) expect(c).not.toMatch(/\btap-target\b/);
    }
  });
});
