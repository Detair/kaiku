import { Component, Show } from "solid-js";
import type { UserStatus, QualityLevel, StatusShape } from "@/lib/types";
import { STATUS_COLORS } from "@/lib/types";
import { t, type MessageKey } from "@/i18n";

interface StatusIndicatorProps {
  /** For user status: online, idle, dnd, offline */
  status?: UserStatus;
  /** For quality indicators: good, warning, poor, unknown */
  quality?: QualityLevel;
  /** Override shape explicitly */
  shape?: StatusShape;
  /** Size variant */
  size?: "xs" | "sm" | "md" | "lg";
  /** Show as overlay on avatar (absolute positioned) */
  overlay?: boolean;
  /** Optional text to show next to indicator (e.g., "42ms") */
  text?: string;
  /** Spoken name; defaults to the status or quality word. */
  label?: string;
  /** The status is already written next to it: hide the shape from screen readers. */
  decorative?: boolean;
}

// A11Y-14: the shape and colour also have a word for screen readers.
const STATUS_LABEL: Record<UserStatus, MessageKey> = {
  online: "home.status.online",
  idle: "home.status.idle",
  dnd: "home.status.dnd",
  offline: "home.status.offline",
};

const QUALITY_LABEL: Record<QualityLevel, MessageKey> = {
  good: "common.quality.good",
  warning: "common.quality.warning",
  poor: "common.quality.poor",
  unknown: "common.quality.unknown",
};

const sizeMap = {
  xs: 8,
  sm: 10,
  md: 12,
  lg: 14,
};

function getShapeForStatus(status: UserStatus): StatusShape {
  switch (status) {
    case "online":
      return "circle";
    case "idle":
      return "triangle";
    case "dnd":
      return "hexagon";
    case "offline":
      return "empty-circle";
  }
}

function getShapeForQuality(quality: QualityLevel): StatusShape {
  switch (quality) {
    case "good":
      return "circle";
    case "warning":
      return "triangle";
    case "poor":
      return "hexagon";
    case "unknown":
      return "empty-circle";
  }
}

function getColorForStatus(status: UserStatus): string {
  switch (status) {
    case "online":
      return STATUS_COLORS.good;
    case "idle":
      return STATUS_COLORS.warning;
    case "dnd":
      return STATUS_COLORS.poor;
    case "offline":
      return STATUS_COLORS.unknown;
  }
}

function getColorForQuality(quality: QualityLevel): string {
  return STATUS_COLORS[quality];
}

const StatusIndicator: Component<StatusIndicatorProps> = (props) => {
  const size = () => sizeMap[props.size ?? "md"];

  const shape = (): StatusShape => {
    if (props.shape) return props.shape;
    if (props.quality) return getShapeForQuality(props.quality);
    if (props.status) return getShapeForStatus(props.status);
    return "circle";
  };

  const color = (): string => {
    if (props.quality) return getColorForQuality(props.quality);
    if (props.status) return getColorForStatus(props.status);
    return STATUS_COLORS.unknown;
  };

  const renderShape = () => {
    const s = size();
    const c = color();
    const sh = shape();

    switch (sh) {
      case "circle":
        return (
          <svg width={s} height={s} viewBox="0 0 12 12">
            <circle cx="6" cy="6" r="5" fill={c} />
          </svg>
        );
      case "triangle":
        return (
          <svg width={s} height={s} viewBox="0 0 12 12">
            <polygon points="6,1 11,10 1,10" fill={c} />
          </svg>
        );
      case "hexagon":
        return (
          <svg width={s} height={s} viewBox="0 0 12 12">
            <polygon
              points="6,1 10.5,3.5 10.5,8.5 6,11 1.5,8.5 1.5,3.5"
              fill={c}
            />
          </svg>
        );
      case "empty-circle":
        return (
          <svg width={s} height={s} viewBox="0 0 12 12">
            <circle
              cx="6"
              cy="6"
              r="4"
              fill="none"
              stroke={c}
              stroke-width="2"
            />
          </svg>
        );
    }
  };

  const label = () => {
    if (props.label) return props.label;
    if (props.quality) return t(QUALITY_LABEL[props.quality]);
    if (props.status) return t(STATUS_LABEL[props.status]);
    return undefined;
  };

  const positionClass = () =>
    props.overlay ? "absolute -bottom-0.5 -right-0.5" : "";

  return (
    <span class={`inline-flex items-center gap-1 ${positionClass()}`}>
      <span
        class="inline-flex"
        role={props.decorative || !label() ? undefined : "img"}
        aria-label={props.decorative ? undefined : label()}
        aria-hidden={props.decorative || !label() ? "true" : undefined}
        title={props.decorative ? undefined : label()}
      >
        {renderShape()}
      </span>
      <Show when={props.text}>
        <span class="text-xs" style={{ color: color() }}>
          {props.text}
        </span>
      </Show>
    </span>
  );
};

export default StatusIndicator;
