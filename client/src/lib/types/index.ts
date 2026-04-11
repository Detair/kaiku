/**
 * Shared Types for Kaiku Client
 *
 * These types mirror the Rust types in shared/vc-common.
 * This file re-exports all domain type modules so consumers can continue
 * importing from `@/lib/types` after the split.
 */

export * from "./common";
export * from "./user";
export * from "./guild";
export * from "./channel";
export * from "./message";
export * from "./voice";
export * from "./auth";
export * from "./admin";
export * from "./pages";
export * from "./social";
export * from "./e2ee";
export * from "./events";
export * from "./preferences";
export * from "./search";
export * from "./settings";
export * from "./favorites";
export * from "./observability";
