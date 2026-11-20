/**
 * Minimum client version announced by the server (`GET /api/settings` →
 * `min_client_version {web, desktop, android}`, each a version or null; the
 * field is missing on older servers). Below the minimum the app shows one
 * calm "please update" hint; it never blocks use. Loaded only when a server
 * sends the field.
 */
import { showToast } from "@/components/ui/Toast";
import { isTauri } from "@/lib/tauri/common";
import { t } from "@/i18n";

export interface MinClientVersion {
  web?: string | null;
  desktop?: string | null;
  android?: string | null;
}

interface Parsed {
  core: [number, number, number];
  pre: string[];
}

function parse(version: string): Parsed | null {
  const m = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+[0-9A-Za-z.-]+)?$/.exec(
    version.trim(),
  );
  if (!m) return null;
  return {
    core: [Number(m[1]), Number(m[2]), Number(m[3])],
    pre: m[4] ? m[4].split(".") : [],
  };
}

/** Semver precedence: <0, 0, >0; null when either side is not a version. */
export function compareVersions(a: string, b: string): number | null {
  const x = parse(a);
  const y = parse(b);
  if (!x || !y) return null;
  for (let i = 0; i < 3; i++) {
    if (x.core[i] !== y.core[i]) return x.core[i] - y.core[i];
  }
  // A pre-release ranks below the release.
  if (x.pre.length === 0 || y.pre.length === 0) return y.pre.length - x.pre.length;
  for (let i = 0; i < Math.min(x.pre.length, y.pre.length); i++) {
    const p = x.pre[i];
    const q = y.pre[i];
    if (p === q) continue;
    const pn = /^\d+$/.test(p);
    const qn = /^\d+$/.test(q);
    if (pn && qn) return Number(p) - Number(q);
    if (pn) return -1;
    if (qn) return 1;
    return p < q ? -1 : 1;
  }
  return x.pre.length - y.pre.length;
}

/** True only when both are versions and `current` is below `minimum`. */
export function isBelowMinimum(current: string, minimum: string): boolean {
  const c = compareVersions(current, minimum);
  return c !== null && c < 0;
}

let shownFor: string | null = null;

/** Forget the shown hint (sign-out, tests). */
export function resetMinVersionNotice(): void {
  shownFor = null;
}

/** Show the update hint once when this client is below the server's minimum. */
export function checkMinClientVersion(
  min: MinClientVersion | null | undefined,
  current: string = __KAIKU_VERSION__,
): void {
  const desktop = isTauri();
  const minimum = (desktop ? min?.desktop : min?.web) ?? null;
  if (!minimum || !isBelowMinimum(current, minimum)) return;
  if (shownFor === minimum) return;
  shownFor = minimum;
  showToast({
    type: "info",
    id: "min-client-version",
    duration: 0,
    title: t("notifications.update.title"),
    message: t(desktop ? "notifications.update.desktop" : "notifications.update.web", {
      version: minimum,
    }),
    action: desktop
      ? {
          label: t("notifications.update.check"),
          onClick: () => {
            void import("@/lib/updater").then(({ checkForUpdate }) => checkForUpdate());
          },
        }
      : { label: t("notifications.update.reload"), onClick: () => window.location.reload() },
  });
}
