/**
 * Whether an error means a piece of the app's code could not be downloaded
 * (a lazily loaded chunk), as opposed to a window that failed while rendering.
 *
 * A failed download needs a reload: the browser remembers a failed dynamic
 * import, so trying again in place cannot help. A render error can be retried.
 * Browsers word the import failure differently, so match each known wording:
 * - Chromium: "Failed to fetch dynamically imported module: <url>"
 * - Firefox: "error loading dynamically imported module: <url>"
 * - Safari / WebKitGTK: "Importing a module script failed."
 * - Vite's preload helper: "Unable to preload CSS for <url>"
 * - webpack-style loaders: name "ChunkLoadError"
 * A bare "Failed to fetch" is a failed request inside a window, not a chunk.
 */

const CHUNK_LOAD_MESSAGE =
  /dynamically imported module|importing a module script failed|unable to preload css/i;

export function isChunkLoadError(err: unknown): boolean {
  if (typeof err !== "object" || err === null) return false;
  const { name, message } = err as { name?: unknown; message?: unknown };
  if (name === "ChunkLoadError") return true;
  return typeof message === "string" && CHUNK_LOAD_MESSAGE.test(message);
}
