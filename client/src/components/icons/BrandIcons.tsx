/**
 * Brand icons removed from `lucide-solid` v1.0 for trademark reasons.
 * Re-implemented locally to preserve UX for OIDC provider login buttons
 * (the original path data from lucide-solid v0.577.0, ISC licensed).
 *
 * Shape matches lucide's icon component prop surface (size, color, stroke-width
 * via class, etc.) — drop-in replacement for the original imports.
 */

import type { JSX, Component } from "solid-js";

type IconProps = {
  size?: number | string;
  color?: string;
  class?: string;
} & Omit<JSX.SvgSVGAttributes<SVGSVGElement>, "color">;

function makeIcon(name: string, children: JSX.Element): Component<IconProps> {
  return (props) => {
    const { size = 24, color = "currentColor", class: cls, ...rest } = props;
    return (
      <svg
        xmlns="http://www.w3.org/2000/svg"
        width={size}
        height={size}
        viewBox="0 0 24 24"
        fill="none"
        stroke={color}
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
        class={`lucide lucide-${name} ${cls ?? ""}`}
        {...rest}
      >
        {children}
      </svg>
    );
  };
}

export const Github = makeIcon(
  "github",
  <>
    <path d="M15 22v-4a4.8 4.8 0 0 0-1-3.5c3 0 6-2 6-5.5.08-1.25-.27-2.48-1-3.5.28-1.15.28-2.35 0-3.5 0 0-1 0-3 1.5-2.64-.5-5.36-.5-8 0C6 2 5 2 5 2c-.3 1.15-.3 2.35 0 3.5A5.403 5.403 0 0 0 4 9c0 3.5 3 5.5 6 5.5-.39.49-.68 1.05-.85 1.65-.17.6-.22 1.23-.15 1.85v4" />
    <path d="M9 18c-4.51 2-5-2-7-2" />
  </>,
);

export const Chrome = makeIcon(
  "chrome",
  <>
    <path d="M10.88 21.94 15.46 14" />
    <path d="M21.17 8H12" />
    <path d="M3.95 6.06 8.54 14" />
    <circle cx="12" cy="12" r="10" />
    <circle cx="12" cy="12" r="4" />
  </>,
);
