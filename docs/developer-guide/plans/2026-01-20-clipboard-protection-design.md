# Clipboard Protection Design

**Date:** 2026-01-20
**Status:** ✅ Implemented (PR #39)
**Approach:** A (Application-Level Protection)

## Overview

Protect users from clipboard-based attacks (hijacking, sniffing, paste injection) while maintaining usability. Designed for gaming community users across trusted home machines, potentially compromised systems, and shared computers (internet cafes).

### Goals

- Prevent clipboard hijacking (malware replacing copied content)
- Prevent clipboard sniffing (exposure of sensitive data)
- Detect paste injection (modified content between copy/paste)
- Maintain smooth UX for common cases
- Provide "Paranoid Mode" for high-security scenarios

### Non-Goals (Future Roadmap)

- **Approach B:** OS-level secure storage (Keychain, Credential Manager, Wayland secure clipboard)
- **Approach C:** Clipboard avoidance (QR codes, encrypted files, mobile companion app)

---

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Frontend (Solid.js)                  │
│  ┌─────────────────┐  ┌─────────────────────────────┐  │
│  │ Copy Actions    │  │ Paste Actions               │  │
│  │ (invoke)        │  │ (invoke)                    │  │
│  └────────┬────────┘  └──────────────┬──────────────┘  │
└───────────┼──────────────────────────┼──────────────────┘
            │                          │
            ▼                          ▼
┌─────────────────────────────────────────────────────────┐
│                 Tauri Commands Layer                    │
│  secure_copy()     secure_paste()     clear_clipboard() │
└───────────────────────────┬─────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                   ClipboardGuard Service                │
│  ┌──────────────┐ ┌──────────────┐ ┌────────────────┐  │
│  │ Sensitivity  │ │ Auto-Clear   │ │ Tamper         │  │
│  │ Classifier   │ │ Scheduler    │ │ Detector       │  │
│  └──────────────┘ └──────────────┘ └────────────────┘  │
│  ┌──────────────┐ ┌──────────────┐                     │
│  │ Audit Log    │ │ Settings     │                     │
│  └──────────────┘ └──────────────┘                     │
└───────────────────────────┬─────────────────────────────┘
                            │
                            ▼
                   System Clipboard API
```

**Key Principle:** All clipboard operations go through `ClipboardGuard`. Direct `navigator.clipboard` calls are prohibited in the frontend (enforced by ESLint).

---

## Core Components

### Sensitivity Classifier

Categorizes clipboard content by risk level:

| Level | Default Timeout | Paranoid Timeout | Examples |
|-------|-----------------|------------------|----------|
| `Critical` | 60s | 30s | Recovery phrases, E2EE keys |
| `Sensitive` | 120s | 30s | Invite links, auth tokens |
| `Normal` | None | None | Message text, usernames |

Classification based on `CopyContext` enum passed to `secure_copy`.

### Auto-Clear Scheduler

```rust
pub struct PendingClear {
    content_hash: [u8; 32],  // SHA-256 of copied content
    clear_at: Instant,
    sensitivity: Sensitivity,
}
```

- Tracks pending clears in memory
- Single background task checks every second
- Only clears if clipboard still contains our content (hash match)
- Cancels pending clear if user copies something else

### Tamper Detector

For `Critical` and `Sensitive` content:

1. On copy: Store `SHA-256(content)` in memory
2. On paste (within app): Read clipboard, hash, compare
3. Mismatch → Block paste, show warning: "Clipboard was modified by another application"

### Audit Log (Paranoid Mode Only)

- Timestamp, operation type, sensitivity level, context
- No content stored — only metadata
- Configurable retention (default: 7 days)

---

## Tauri Commands API

### Commands

```rust
#[derive(Serialize, Deserialize, Clone)]
pub enum CopyContext {
    RecoveryPhrase,
    InviteLink,
    MessageContent,
    UserId,
    Other(String),
}

#[derive(Serialize, Deserialize, Clone)]
pub enum Sensitivity {
    Critical,
    Sensitive,
    Normal,
}

#[tauri::command]
async fn secure_copy(
    content: String,
    sensitivity: Sensitivity,
    context: CopyContext,
) -> Result<CopyResult, ClipboardError>;

#[tauri::command]
async fn secure_paste(
    expected_context: Option<CopyContext>,
) -> Result<PasteResult, ClipboardError>;

#[tauri::command]
async fn clear_clipboard() -> Result<(), ClipboardError>;

#[tauri::command]
async fn extend_clipboard_timeout(additional_secs: u32) -> Result<(), ClipboardError>;
```

### Response Types

```rust
pub struct CopyResult {
    pub success: bool,
    pub auto_clear_in_secs: Option<u32>,
}

pub struct PasteResult {
    pub content: String,
    pub tampered: bool,
    pub external: bool,  // True if content wasn't copied by our app
    pub context: Option<CopyContext>,
}

pub enum ClipboardError {
    AccessDenied,
    TamperDetected,
    Cleared,
    MaxExtensionsReached,
    PermissionDenied,
}
```

### Events

```rust
#[derive(Clone, Serialize)]
pub struct ClipboardStatusEvent {
    pub has_sensitive_content: bool,
    pub clear_in_secs: Option<u32>,
    pub context: Option<CopyContext>,
}

// Emitted on clipboard state changes
app.emit("clipboard-status", status);
app.emit("clipboard-tamper-detected", TamperEvent { context });
```

---

## UI Components

### Copy Confirmation Toast

```
┌─────────────────────────────────────────┐
│ 🔒 Copied securely                      │
│ Recovery phrase • Auto-clears in 30s    │
│ ████████████████░░░░  [Extend] [Clear]  │
└─────────────────────────────────────────┘
```

- Progress bar shows time remaining
- "Extend" adds 30s (max 2 extensions, disabled in paranoid mode)
- "Clear" immediately wipes clipboard
- Toast persists until cleared or timeout

### Tamper Warning Modal

```
┌───────────────────────────────────────────────┐
│ ⚠️  Clipboard Modified                        │
│                                               │
│ The clipboard content was changed after you   │
│ copied it. This could indicate malware.       │
│                                               │
│ What you copied:  Invite link                 │
│ Action blocked:   Paste                       │
│                                               │
│ [Paste Anyway (Risky)]  [Cancel]  [Learn More]│
└───────────────────────────────────────────────┘
```

- Default action is Cancel (Enter key)
- "Paste Anyway" requires explicit click (no keyboard shortcut)

### Clipboard Status Indicator

```
[ 🔒 28s ] ← Click to clear immediately
```

- Small icon in app header when sensitive content on clipboard
- Single click clears clipboard
- Always visible in Strict/Paranoid mode

### Paranoid Mode Copy Dialog

```
┌───────────────────────────────────────────────┐
│ 📋 Copy Recovery Phrase?                      │
│                                               │
│ Copying to clipboard exposes this phrase to   │
│ other applications on your computer.          │
│                                               │
│ Alternatives:                                 │
│ • Write it down manually (recommended)        │
│ • Export encrypted backup file                │
│ • [Future] Transfer via mobile app            │
│                                               │
│ [Copy Anyway (30s)]  [Show QR]  [Cancel]      │
└───────────────────────────────────────────────┘
```

---

## Settings

### Schema

```rust
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ClipboardSettings {
    pub protection_level: ProtectionLevel,
    pub paranoid_mode_enabled: bool,
    pub show_copy_toast: bool,
    pub show_status_indicator: bool,
    pub audit_log_retention_days: u32,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub enum ProtectionLevel {
    Minimal,
    #[default]
    Standard,
    Strict,
}
```

### Protection Level Matrix

| Level | Critical Timeout | Sensitive Timeout | Tamper Action | Indicator |
|-------|------------------|-------------------|---------------|-----------|
| Minimal | None | None | Warn only | When copying |
| Standard | 60s | 120s | Block | When sensitive |
| Strict | 30s | 60s | Block | Always |
| Paranoid | 30s | 30s | Block + audit | Always + confirmation |

### Settings UI

```
Security → Clipboard Protection
┌─────────────────────────────────────────────────┐
│ Protection Level                                │
│ ○ Minimal    - No auto-clear                    │
│ ● Standard   - Auto-clear sensitive data (rec.) │
│ ○ Strict     - Shorter timeouts, always visible │
│                                                 │
│ ☑ Show copy confirmation                        │
│ ☑ Show clipboard indicator                      │
│                                                 │
│ ─────────────────────────────────────────────── │
│ 🛡️ Paranoid Mode                    [Disabled ▾]│
│ Extra protection for shared computers           │
│                                                 │
│                          [Reset to Defaults]    │
└─────────────────────────────────────────────────┘
```

---

## Paranoid Mode

| Behavior | Normal Mode | Paranoid Mode |
|----------|-------------|---------------|
| Critical content copy | Allowed, 60s timeout | Confirmation dialog required |
| Sensitive content copy | Allowed, 120s timeout | Allowed, 30s timeout |
| Auto-clear extensions | Up to 2 extensions | No extensions allowed |
| Tamper detection | Block + warning | Block + warning + audit log |
| Clipboard status indicator | Configurable | Always visible |
| Copy confirmation toast | Dismissible | Persists until cleared |
| External paste | Allowed | Warning shown first |
| Audit logging | Disabled | Enabled |

### First-Enable Modal

```
┌───────────────────────────────────────────────┐
│ 🛡️ Paranoid Mode                              │
│                                               │
│ Recommended if you:                           │
│ • Use shared or public computers              │
│ • Want maximum protection for E2EE keys       │
│ • Suspect your system may be compromised      │
│                                               │
│ What changes:                                 │
│ • Extra confirmation for sensitive copies     │
│ • Shorter clipboard timeouts                  │
│ • Warnings when pasting external content      │
│ • Security audit log enabled                  │
│                                               │
│ [Enable Paranoid Mode]  [Cancel]              │
└───────────────────────────────────────────────┘
```

---

## Migration Path

### Current Clipboard Usage

- `client/src/components/guilds/InvitesTab.tsx:53` — `navigator.clipboard.writeText(url)`

### Migration Steps

1. **Add Tauri Clipboard Capability**

```json
// client/src-tauri/capabilities/default.json
{
  "permissions": [
    "clipboard:default",
    "clipboard:allow-read",
    "clipboard:allow-write"
  ]
}
```

2. **Implement ClipboardGuard (Rust)**

3. **Create Frontend Library**

```typescript
// client/src/lib/clipboard.ts
import { invoke } from '@tauri-apps/api/core';

const isTauri = '__TAURI__' in window;

export async function secureCopy(
  content: string,
  sensitivity: Sensitivity,
  context: CopyContext
): Promise<CopyResult> {
  if (isTauri) {
    return invoke('secure_copy', { content, sensitivity, context });
  }
  await navigator.clipboard.writeText(content);
  return { success: true, autoClearInSecs: null };
}
```

4. **Migrate Existing Code**

Before:
```typescript
await navigator.clipboard.writeText(url);
```

After:
```typescript
import { secureCopy, CopyContext } from '@/lib/clipboard';
await secureCopy(url, 'sensitive', CopyContext.InviteLink);
```

5. **Add ESLint Rule**

```javascript
{
  rules: {
    'no-restricted-syntax': ['error', {
      selector: "MemberExpression[object.name='navigator'][property.name='clipboard']",
      message: 'Use @/lib/clipboard for clipboard operations'
    }]
  }
}
```

6. **Verification Script**

```bash
#!/bin/bash
grep -r "navigator.clipboard" client/src --include="*.ts" --include="*.tsx" \
  | grep -v "lib/clipboard.ts" \
  && echo "FAIL: Direct clipboard access found" && exit 1 \
  || echo "PASS: All clipboard access goes through ClipboardGuard"
```

### UI Responsibility Split

| Feedback Type | Owner |
|---------------|-------|
| Security toast (timeout, warnings) | ClipboardGuard global |
| Action confirmation (button state) | Component local |
| Tamper modal | ClipboardGuard global |

---

## Extension Points

Minimal hooks for future Approach B/C, documented but not implemented:

```rust
impl ClipboardGuard {
    async fn write_clipboard(&self, content: &str) -> Result<(), ClipboardError> {
        // Extension: Extract to ClipboardBackend trait when adding OS-specific backends
        arboard::Clipboard::new()?.set_text(content)?;
        Ok(())
    }

    fn should_avoid_clipboard(&self, context: &CopyContext) -> bool {
        // Extension: Check for available alternatives (QR, mobile companion)
        self.settings.paranoid_mode_enabled
            && matches!(context, CopyContext::RecoveryPhrase)
    }
}
```

### Future Extensions (Not Implemented)

**Approach B: OS Integration**
- macOS: Keychain for recovery phrases
- Windows: Credential Manager
- Linux/Wayland: Secure clipboard portal

**Approach C: Clipboard Avoidance**
- QR code display + camera scan
- Encrypted file export/import
- Mobile companion app (protocol TBD)

---

## Testing Strategy

### Unit Tests (Rust)

```rust
#[test]
fn test_sensitivity_classification() {
    assert_eq!(classify_context(CopyContext::RecoveryPhrase), Sensitivity::Critical);
}

#[test]
fn test_tamper_detection() {
    let hash = compute_hash("secret");
    assert!(verify_hash("secret", &hash));
    assert!(!verify_hash("modified", &hash));
}

#[tokio::test]
async fn test_auto_clear_scheduler() {
    let guard = ClipboardGuard::new_for_test();
    guard.schedule_clear(hash, Duration::from_millis(50)).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(guard.pending_clears.lock().await.is_empty());
}
```

### Error Path Tests

```rust
#[tokio::test]
async fn test_paste_after_clear_returns_error() {
    let guard = ClipboardGuard::new_for_test();
    guard.copy("secret", Sensitivity::Critical, CopyContext::RecoveryPhrase).await.unwrap();
    guard.clear().await.unwrap();

    let result = guard.paste().await;
    assert!(matches!(result, Err(ClipboardError::Cleared)));
}

#[tokio::test]
async fn test_extend_timeout_respects_max() {
    let guard = ClipboardGuard::new_for_test();
    guard.extend_timeout(30).await.unwrap();
    guard.extend_timeout(30).await.unwrap();
    assert!(matches!(guard.extend_timeout(30).await, Err(ClipboardError::MaxExtensionsReached)));
}
```

### Security Tests

```rust
#[test]
fn test_audit_log_no_sensitive_content() {
    let entry = AuditLogEntry::new(CopyContext::RecoveryPhrase, Sensitivity::Critical, AuditAction::Copy);
    let serialized = serde_json::to_string(&entry).unwrap();
    assert!(!serialized.contains("actual-secret-phrase"));
}
```

### Concurrency Tests

```rust
#[tokio::test]
async fn test_rapid_copy_replaces_pending_clear() {
    let guard = ClipboardGuard::new_for_test();
    for i in 0..10 {
        guard.copy(&format!("content-{}", i), Sensitivity::Sensitive, CopyContext::InviteLink).await.unwrap();
    }
    assert_eq!(guard.pending_clears.lock().await.len(), 1);
}
```

### Frontend Tests (Vitest)

```typescript
describe('secureCopy', () => {
  it('calls Tauri command with correct params', async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    (invoke as any).mockResolvedValue({ success: true, autoClearInSecs: 60 });

    const result = await secureCopy('test', 'sensitive', 'InviteLink');
    expect(invoke).toHaveBeenCalledWith('secure_copy', {
      content: 'test',
      sensitivity: 'sensitive',
      context: 'InviteLink',
    });
  });
});
```

### Manual QA Checklist

**Basic Flow**
- [ ] Copy invite link → toast appears with countdown
- [ ] Wait for full timeout → verify clipboard empty (paste in Notepad/TextEdit)
- [ ] Click "Clear Now" → immediate clear
- [ ] Click "Extend" → timer resets

**Tamper Detection**
- [ ] Copy in app → modify in external app → paste in app → warning modal
- [ ] Click "Cancel" on warning → paste blocked
- [ ] Click "Paste Anyway" → paste proceeds

**Paranoid Mode**
- [ ] Enable → confirmation dialog on recovery phrase copy
- [ ] "Copy Anyway" → copies with 30s timeout
- [ ] "Cancel" → nothing copied
- [ ] Audit log entry created

**Edge Cases**
- [ ] Rapid copy (10x fast clicks) → only latest content, one timeout
- [ ] Copy, switch apps, wait timeout, return → clipboard cleared
- [ ] Copy, close app before timeout → clipboard NOT auto-cleared (expected)

**Platforms**
- [ ] Windows 10/11
- [ ] macOS 12+
- [ ] Ubuntu 22.04 (X11)
- [ ] Ubuntu 22.04 (Wayland)

---

## File Structure

```
client/src-tauri/src/
├── commands/
│   └── clipboard.rs              # Tauri commands
├── services/
│   └── clipboard_guard.rs        # Core logic
├── settings/
│   └── clipboard.rs              # Settings schema
└── main.rs                       # Register commands

client/src/
├── lib/
│   └── clipboard.ts              # Frontend wrapper
└── components/clipboard/
    ├── ClipboardToast.tsx        # Copy confirmation
    ├── ClipboardIndicator.tsx    # Header status
    ├── TamperWarningModal.tsx    # Tamper detection
    └── ParanoidCopyDialog.tsx    # Paranoid mode confirmation
```

---

## Implementation Order

1. `ClipboardGuard` service (Rust core logic)
2. Tauri commands + capability
3. Frontend `clipboard.ts` library
4. `ClipboardToast` component
5. Migrate `InvitesTab.tsx`
6. `ClipboardIndicator` component
7. `TamperWarningModal` component
8. Settings UI
9. Paranoid mode + `ParanoidCopyDialog`
10. ESLint rule enforcement
11. Testing + QA
