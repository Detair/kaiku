/**
 * Vitest setup: merge the on-demand English areas (`locale.ts` `LAZY_AREAS`)
 * before any test calls `t()`. The app does this in `initLocale`.
 */
import { loadLazyAreas } from "./locale";

if (!(await loadLazyAreas())) throw new Error("i18n: on-demand English areas did not load");
