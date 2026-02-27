import { Component, ErrorBoundary, JSX, ParentProps } from "solid-js";

const Spinner: Component<{ class?: string }> = (props) => (
  <div
    class={`animate-spin rounded-full border-2 border-white/10 border-t-accent-primary ${props.class ?? "w-8 h-8"}`}
  />
);

/** Centered spinner for route-level lazy components */
export const PageFallback: Component = () => (
  <div class="flex-1 flex items-center justify-center h-screen">
    <Spinner />
  </div>
);

/** Smaller spinner for modal-level lazy components */
export const ModalFallback: Component = () => (
  <div class="flex items-center justify-center p-8">
    <Spinner class="w-6 h-6" />
  </div>
);

/** Error boundary for lazy-loaded components — catches chunk load failures with retry */
export const LazyErrorBoundary: Component<
  ParentProps<{ name?: string; fallback?: JSX.Element }>
> = (props) => (
  <ErrorBoundary
    fallback={(err, reset) => {
      console.error(
        `[LazyLoad] Failed to load "${props.name ?? "component"}":`,
        err,
      );
      return (
        props.fallback ?? (
          <div class="flex flex-col items-center justify-center gap-3 p-6 text-text-secondary">
            <p class="text-sm">Failed to load this section.</p>
            <button
              class="px-4 py-2 text-sm bg-accent-primary/20 text-accent-primary rounded-lg hover:bg-accent-primary/30 transition-colors"
              onClick={reset}
            >
              Try again
            </button>
          </div>
        )
      );
    }}
  >
    {props.children}
  </ErrorBoundary>
);
