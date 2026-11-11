import { Component, Show } from "solid-js";
import type { UserStatus } from "@/lib/types";
import StatusIndicator from "./StatusIndicator";

interface AvatarProps {
  src?: string | null;
  alt: string;
  size?: "xs" | "sm" | "md" | "lg";
  status?: UserStatus;
  showStatus?: boolean;
}

const sizeClasses = {
  xs: "w-5 h-5 text-[10px]",
  sm: "w-8 h-8 text-xs",
  md: "w-10 h-10 text-sm",
  lg: "w-12 h-12 text-base",
};

const Avatar: Component<AvatarProps> = (props) => {
  const size = () => props.size ?? "md";

  // Get initials from name
  const initials = () => {
    const name = props.alt || "?";
    const parts = name.split(" ");
    if (parts.length >= 2) {
      return (parts[0][0] + parts[1][0]).toUpperCase();
    }
    return name.slice(0, 2).toUpperCase();
  };

  // Generate a consistent color from the name. The -700 shades keep the white
  // initials at 4.5:1 or better (the -500 shades were about 2:1 for yellow/lime).
  const bgColor = () => {
    const colors = [
      "bg-red-700",
      "bg-orange-700",
      "bg-amber-700",
      "bg-yellow-700",
      "bg-lime-700",
      "bg-green-700",
      "bg-emerald-700",
      "bg-teal-700",
      "bg-cyan-700",
      "bg-sky-700",
      "bg-blue-700",
      "bg-indigo-700",
      "bg-violet-700",
      "bg-purple-700",
      "bg-fuchsia-700",
      "bg-pink-700",
    ];
    const name = props.alt || "?";
    let hash = 0;
    for (let i = 0; i < name.length; i++) {
      hash = name.charCodeAt(i) + ((hash << 5) - hash);
    }
    return colors[Math.abs(hash) % colors.length];
  };

  return (
    <div class="relative inline-block">
      <Show
        when={props.src}
        fallback={
          <div
            class={`${sizeClasses[size()]} ${bgColor()} rounded-full flex items-center justify-center text-white font-medium`}
          >
            {initials()}
          </div>
        }
      >
        <img
          src={props.src!}
          alt={props.alt}
          class={`${sizeClasses[size()]} rounded-full object-cover`}
        />
      </Show>
      <Show when={props.showStatus && props.status}>
        <StatusIndicator status={props.status!} size={size()} overlay />
      </Show>
    </div>
  );
};

export default Avatar;
