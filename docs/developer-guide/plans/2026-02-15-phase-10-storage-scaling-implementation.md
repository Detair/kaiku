# Phase 10 Storage SaaS Scaling Architecture - Implementation Plan

**Date:** 2026-02-15
**Status:** Draft
**Design Reference:** `docs/plans/2026-02-15-phase-10-storage-scaling-design.md`

## Objective

Implement signed media delivery and CDN integration to support SaaS-scale attachment and media workloads without degrading security posture.

## Open Standards Enforcement

- Signed URL/cookie flow must remain S3/SigV4-compatible.
- Cache semantics follow standard HTTP cache-control behavior.
- Telemetry uses OpenTelemetry metric and trace conventions.

## Implementation Phases

### Phase A - Signing service and policy

1. Add signing service abstraction and key rotation support.
2. Add authorization checks before signing.
3. Add TTL and scope policy per media type and access context.
4. Add tests for expired/forged/mismatched signatures.

### Phase B - CDN integration

1. Add CDN endpoint selection and cache policy mapping.
2. Add invalidation hooks for deletes/access changes.
3. Add fallback behavior when CDN unavailable.
4. Add load tests for cache miss/hit profiles.

### Phase C - Rollout and hardening

1. Add feature flag and staged rollout path.
2. Add monitoring dashboards (latency, hit rate, egress).
3. Add rollback runbook to previous proxy path.
4. Validate cost and latency improvements before GA.

## File Targets

- `server/src/chat/uploads.rs`
- `server/src/chat/s3.rs`
- `server/src/config.rs`
- `server/tests/uploads_http_test.rs`
- `infra/cdn/`
- `docs/ops/`

## Verification

- signed-url tests pass for auth and expiry boundaries
- CDN integration tests and rollback test pass
- telemetry dashboards show expected latency/hit-rate movement

## Done Criteria

- Storage scaling path is standards-compliant, test-covered, and operationally reversible.
