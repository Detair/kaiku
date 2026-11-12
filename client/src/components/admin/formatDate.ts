import { formatLocales } from "@/i18n/format";

/** Day, month and year in the reader's language (not fixed `en-US`, ADMIN-23). */
export function formatDay(iso: string): string {
  return new Date(iso).toLocaleDateString(formatLocales(), {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}
