import { Component, Show } from "solid-js";
import type { UserStatus } from "@/lib/types";
import StatusIndicator from "./StatusIndicator";

interface AvatarProps {
  src?: string | null;
  alt: string;
  size?: "sm" | "md" | "lg";
  status?: UserStatus;
  showStatus?: boolean;
}

const sizeClasses = {
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

  // Generate a consistent color from the name
  const bgColor = () => {
    const colors = [
      "bg-red-500",
      "bg-orange-500",
      "bg-amber-500",
      "bg-yellow-500",
      "bg-lime-500",
      "bg-green-500",
      "bg-emerald-500",
      "bg-teal-500",
      "bg-cyan-500",
      "bg-sky-500",
      "bg-blue-500",
      "bg-indigo-500",
      "bg-violet-500",
      "bg-purple-500",
      "bg-fuchsia-500",
      "bg-pink-500",
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
        <StatusIndicator status={props.status!} size={size()} />
      </Show>
    </div>
  );
};

export default Avatar;
