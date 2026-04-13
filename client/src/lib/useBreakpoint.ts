import { createSignal, onCleanup } from "solid-js";

export function useBreakpoint(query: string): () => boolean {
  const mql = window.matchMedia(query);
  const [matches, setMatches] = createSignal(mql.matches);
  const handler = (e: MediaQueryListEvent) => setMatches(e.matches);
  mql.addEventListener("change", handler);
  onCleanup(() => mql.removeEventListener("change", handler));
  return matches;
}

export function useIsMobile(): () => boolean {
  return useBreakpoint("(max-width: 767px)");
}
