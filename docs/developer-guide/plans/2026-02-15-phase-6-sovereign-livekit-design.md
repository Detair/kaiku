# Phase 6 Sovereign Guild and Live Session Toolkit - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Scope:** Phase 6 `[Content] Sovereign Guild Model (BYO Infrastructure)`, `[Voice] Live Session Toolkits`

## Problem

Privacy-sensitive communities need customer-controlled storage and relay paths, while power users need richer in-voice collaboration features for raids and work sessions.

## Goals

- Define a sovereign guild profile model with BYO storage and relay options.
- Define live-session toolkit primitives with role-aware access controls.
- Preserve low-latency voice characteristics and clear operational safety boundaries.

## Non-Goals

- Full federation protocol support in this phase.
- Unlimited provider permutations in v1.
- Complex plugin runtime execution for live tools in first release.

## Open Standards Profile

- **Storage API:** S3-compatible API + SigV4 request signing.
- **Voice Transport:** WebRTC (ICE/STUN/TURN, RTP/RTCP) using standards-compliant negotiation.
- **Realtime Control:** WebSocket RFC 6455 for toolkit state sync.
- **Config Contracts:** JSON Schema for provider and profile validation.
- **Telemetry:** OpenTelemetry for relay quality and toolkit operation events.

## Architecture (High Level)

- Extend deployment profile model with `sovereign` capabilities:
  - BYO object storage endpoint and credentials reference.
  - BYO relay/SFU endpoint configuration.
  - Policy checks for region and trust constraints.
- Add live toolkit service layer:
  - Timer orchestration events.
  - Session notes buffer and summary publishing.
  - Action-item model with final channel post.

## Security and Governance

- Strict validation of external endpoint configuration.
- Scoped credentials and rotation guidance for BYO providers.
- Role-based controls for toolkit actions and visibility.
- Audit events for profile/config and session-tool changes.

## Performance Constraints

- Keep toolkit state sync lightweight and event-driven.
- Avoid blocking voice signaling paths with toolkit logic.
- Measure relay and session toolkit overhead under load.

## Success Criteria

- Sovereign guild profile validates and activates safely.
- Live session toolkit supports gaming and work modes with permissions.
- No measurable regressions in voice setup/reconnect baselines.

## References

- `docs/project/roadmap.md`
- `docs/plans/2026-02-15-sovereign-byo-deployment-design.md`
