/**
 * Skeleton - Loading placeholder component with pulse animation
 *
 * Provides visual feedback during content loading.
 */

import { Component, JSX, splitProps } from "solid-js";

interface SkeletonProps extends JSX.HTMLAttributes<HTMLDivElement> {
  /** Width of the skeleton (CSS value) */
  width?: string;
  /** Height of the skeleton (CSS value) */
  height?: string;
  /** Whether to use circular shape */
  circle?: boolean;
}

const Skeleton: Component<SkeletonProps> = (props) => {
  const [local, others] = splitProps(props, [
    "width",
    "height",
    "circle",
    "class",
  ]);

  const style = () => ({
    width: local.width,
    height: local.height,
  });

  return (
    <div
      class={`animate-pulse bg-white/10 ${local.circle ? "rounded-full" : "rounded"} ${local.class || ""}`}
      style={style()}
      {...others}
    />
  );
};

export default Skeleton;
