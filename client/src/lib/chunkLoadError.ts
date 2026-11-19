/**
 * Telling "the window's code could not be loaded" apart from "the window
 * failed while rendering".
 *
 * The two need different words and actions: a failed load is about the
 * connection (Try again runs the import again, since Solid's `lazy` drops a
 * rejected import, and Reload is offered too); a render error is about the
 * window. The reliable signal is where it failed:
 * `lazyWindow` tags every rejection of its import as a `ChunkLoadError`,
 * whatever the browser called it (a network error, a chunk answered with an
 * HTML page, a chunk that does not parse).
 */

import { lazy, type Component } from "solid-js";

/** A window's code that could not be downloaded or evaluated. */
export class ChunkLoadError extends Error {
  override name = "ChunkLoadError";
  constructor(cause: unknown) {
    super(cause instanceof Error ? cause.message : String(cause), { cause });
  }
}

/** Wrap an import so any rejection comes out tagged as a `ChunkLoadError`. */
export function loadOrTag<T>(load: () => Promise<T>): () => Promise<T> {
  return () =>
    load().catch((err: unknown) => {
      throw new ChunkLoadError(err);
    });
}

/** `lazy` for a window mounted in `LazyDialog`: a failed import is tagged. Mounting it again loads it again. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
export function lazyWindow<T extends Component<any>>(load: () => Promise<{ default: T }>) {
  return lazy(loadOrTag(load));
}

/**
 * Fallback for errors that did not come through `lazyWindow`: how browsers
 * word a failed dynamic import (Chromium "Failed to fetch dynamically imported
 * module", Firefox "error loading dynamically imported module", Safari
 * "Importing a module script failed.", Vite "Unable to preload CSS for").
 * A bare "Failed to fetch" is a failed request inside a window, not a chunk.
 */
const CHUNK_LOAD_MESSAGE =
  /dynamically imported module|importing a module script failed|unable to preload css/i;

export function isChunkLoadError(err: unknown): boolean {
  if (err instanceof ChunkLoadError) return true;
  if (typeof err !== "object" || err === null) return false;
  const { name, message } = err as { name?: unknown; message?: unknown };
  if (name === "ChunkLoadError") return true;
  return typeof message === "string" && CHUNK_LOAD_MESSAGE.test(message);
}
