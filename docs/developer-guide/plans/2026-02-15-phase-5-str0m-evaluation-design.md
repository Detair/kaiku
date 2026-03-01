# Phase 5 WebRTC Engine Evaluation (str0m) - Design

**Date:** 2026-02-15
**Status:** Draft
**Roadmap Scope:** Phase 5 `[Voice] Evaluate str0m as WebRTC Alternative`

## Problem

The current WebRTC stack is functional but complex under high load and advanced media scenarios. We need a decision framework for whether `str0m` improves performance, maintainability, and protocol correctness enough to justify migration.

## Goals

- Define objective evaluation criteria for `webrtc-rs` vs `str0m`.
- Measure impact on latency, reconnect behavior, and implementation complexity.
- Produce a go/no-go decision record with rollback conditions.

## Non-Goals

- Full migration during evaluation.
- Introducing protocol incompatibilities for existing clients.

## Open Standards Profile

- WebRTC interoperability via ICE/STUN/TURN and RTP/RTCP.
- SDP negotiation compatibility with browser implementations.
- Telemetry via OpenTelemetry metrics/traces for benchmark scenarios.

## Approach

Run controlled benchmark scenarios (DM call, guild voice channel, screen share, reconnect storm) in staging and compare:
- setup latency and reconnect success,
- CPU/memory under concurrent sessions,
- code complexity and maintainability cost,
- standards compliance behavior in interop tests.

## Success Criteria

- Decision made with evidence-backed scorecard.
- Risks, migration path, and rollback strategy documented.

## References

- `docs/project/roadmap.md`
- `server/src/voice/`
