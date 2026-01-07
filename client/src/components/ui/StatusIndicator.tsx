import { Component } from "solid-js";
import type { UserStatus } from "@/lib/types";

interface StatusIndicatorProps {
  status: UserStatus;
  size?: "sm" | "md" | "lg";
}

const statusColors: Record<UserStatus, string> = {
  online: "bg-green-500",
  away: "bg-yellow-500",
  busy: "bg-red-500",
  offline: "bg-gray-500",
};

const sizeClasses = {
  sm: "w-2.5 h-2.5 border-2",
  md: "w-3 h-3 border-2",
  lg: "w-3.5 h-3.5 border-2",
};

const positionClasses = {
  sm: "-bottom-0.5 -right-0.5",
  md: "-bottom-0.5 -right-0.5",
  lg: "-bottom-0.5 -right-0.5",
};

const StatusIndicator: Component<StatusIndicatorProps> = (props) => {
  const size = () => props.size ?? "md";

  return (
    <span
      class={`absolute ${positionClasses[size()]} ${sizeClasses[size()]} ${
        statusColors[props.status]
      } rounded-full border-background-secondary`}
      title={props.status}
    />
  );
};

export default StatusIndicator;
