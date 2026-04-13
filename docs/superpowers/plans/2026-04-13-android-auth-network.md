# Android Auth & Network Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden Android auth security (OIDC, token storage, TLS) and fix network robustness issues (WebSocket reconnect, connectivity detection).

**Architecture:** Nine targeted fixes across auth, network, and DI layers. The WebSocket auth migration (#1) requires a coordinated server+client change with backwards compatibility. All other fixes are client-only.

**Tech Stack:** Kotlin, OkHttp, Ktor, Hilt, EncryptedSharedPreferences, WebSocket

**Spec:** `docs/superpowers/specs/2026-04-13-mobile-implementation-fixes-design.md` — Sub-Project 1

**Branch:** `fix/android-auth-network`

---

## File Map

| File | Responsibility |
|------|---------------|
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/KaikuWebSocket.kt` | WebSocket auth, token expiry check, scope tracking |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ConnectivityMonitor.kt` | NET_CAPABILITY_VALIDATED, Closeable |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/api/KaikuHttpClient.kt` | TLS 1.3 via shared OkHttp config |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/auth/OidcHandler.kt` | PKCE + state nonce |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/AuthRepository.kt` | Defer token persistence until getMe |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/di/NetworkModule.kt` | TLS 1.3 ConnectionSpec, ProviderInstaller |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/di/WebSocketModule.kt` | TLS 1.3 for WebSocket OkHttpClient |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/KaikuApplication.kt` | ProviderInstaller in onCreate |
| `mobile/android/app/src/main/AndroidManifest.xml` | App Links intent filter, BLUETOOTH maxSdkVersion |
| `server/src/ws/events.rs` | Authenticate ClientEvent variant |
| `server/src/ws/handlers.rs` | Dual-auth support (header + frame) |
| `mobile/android/app/src/test/java/io/wolftown/kaiku/data/ws/KaikuWebSocketTest.kt` | Updated auth tests |

---

## Task 1: TLS 1.3 enforcement (#2)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/di/NetworkModule.kt:30`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/di/WebSocketModule.kt:29`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/KaikuApplication.kt:6`

- [ ] **Step 1: Add ProviderInstaller to KaikuApplication**

At `KaikuApplication.kt`, add an `onCreate` override that calls `ProviderInstaller.installIfNeeded(this)` to ensure TLS 1.3 is available on API 26-28:

```kotlin
import android.app.Application
import com.google.android.gms.security.ProviderInstaller
import dagger.hilt.android.HiltAndroidApp
import java.util.logging.Logger

@HiltAndroidApp
class KaikuApplication : Application() {
    private val logger = Logger.getLogger("KaikuApplication")

    override fun onCreate() {
        super.onCreate()
        try {
            ProviderInstaller.installIfNeeded(this)
        } catch (e: Exception) {
            logger.warning("Failed to install security provider: ${e.message}")
        }
    }
}
```

- [ ] **Step 2: Create TLS 1.3 ConnectionSpec in NetworkModule**

At `NetworkModule.kt`, add a `@Provides @Singleton` function for the `ConnectionSpec` and apply it to the HTTP client:

```kotlin
import okhttp3.ConnectionSpec
import okhttp3.TlsVersion

@Provides
@Singleton
fun provideTls13Spec(): ConnectionSpec {
    return ConnectionSpec.Builder(ConnectionSpec.MODERN_TLS)
        .tlsVersions(TlsVersion.TLS_1_3)
        .build()
}
```

Update the existing OkHttp engine provision to use this spec. The Ktor `OkHttp.create()` call at line 30 needs the engine configured with `.config { connectionSpecs(listOf(tls13Spec)) }`.

- [ ] **Step 3: Apply TLS 1.3 spec to WebSocket OkHttpClient**

At `WebSocketModule.kt:29`, inject the `ConnectionSpec` and apply it:

```kotlin
@Provides
@Singleton
fun provideWebSocketClient(tls13Spec: ConnectionSpec): OkHttpClient {
    return OkHttpClient.Builder()
        .connectionSpecs(listOf(tls13Spec))
        .pingInterval(0, TimeUnit.SECONDS)
        .readTimeout(0, TimeUnit.SECONDS)
        .build()
}
```

- [ ] **Step 4: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS — TLS spec is configuration-only, no behavioral change in tests

- [ ] **Step 5: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/di/NetworkModule.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/di/WebSocketModule.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/KaikuApplication.kt
git commit -m "fix(client): enforce TLS 1.3 on all Android network connections (#2)"
```

---

## Task 2: ConnectivityMonitor fixes (#15, #18)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ConnectivityMonitor.kt`

- [ ] **Step 1: Add NET_CAPABILITY_VALIDATED and Closeable**

In `ConnectivityMonitor.kt`:
1. Add `NET_CAPABILITY_VALIDATED` alongside `NET_CAPABILITY_INTERNET` in `checkCurrentConnectivity()` (line 46) and in `onCapabilitiesChanged` (inside the NetworkCallback).
2. Implement `Closeable` interface with a `close()` method that calls `connectivityManager.unregisterNetworkCallback(networkCallback)`.

```kotlin
import java.io.Closeable

@Singleton
class ConnectivityMonitor @Inject constructor(
    @ApplicationContext private val context: Context
) : Closeable {
    // ... existing code ...

    // In checkCurrentConnectivity:
    // Change: hasCapability(NET_CAPABILITY_INTERNET)
    // To:     hasCapability(NET_CAPABILITY_INTERNET) && hasCapability(NET_CAPABILITY_VALIDATED)

    // In onCapabilitiesChanged:
    // Add same NET_CAPABILITY_VALIDATED check

    override fun close() {
        try {
            connectivityManager.unregisterNetworkCallback(networkCallback)
        } catch (_: IllegalArgumentException) {
            // Already unregistered
        }
        _isConnected.value = false
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ConnectivityMonitor.kt
git commit -m "fix(client): require NET_CAPABILITY_VALIDATED and add Closeable to ConnectivityMonitor (#15, #18)"
```

---

## Task 3: WebSocket token expiry check and scope tracking (#16, #17)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/KaikuWebSocket.kt:57,75,123`

- [ ] **Step 1: Track connectivity collector job**

At `KaikuWebSocket.kt:57`, add a field:

```kotlin
private var connectivityJob: Job? = null
```

At `setConnectivityMonitor` (line 75), cancel any existing collector before launching a new one:

```kotlin
fun setConnectivityMonitor(monitor: ConnectivityMonitor) {
    connectivityJob?.cancel()
    connectivityJob = scope.launch {
        monitor.isConnected.collect { connected ->
            // ... existing reconnect logic
        }
    }
}
```

In `disconnect()`, add `connectivityJob?.cancel()` alongside the existing `pingJob?.cancel()` and `reconnectJob?.cancel()`.

- [ ] **Step 2: Add token expiry check before reconnect**

At `doConnect()` (line 123), before building the WebSocket request, add:

```kotlin
if (tokenStorage.isAccessTokenExpired()) {
    _connectionState.value = ConnectionState.TokenExpired
    return
}
```

Add `TokenExpired` to the `ConnectionState` enum at `KaikuWebSocket.kt:28-32` (top-level enum in the same file).

- [ ] **Step 3: Update existing WebSocket test**

At `KaikuWebSocketTest.kt`, update the test `connect uses Sec-WebSocket-Protocol header with token` (line 176) to account for the new `TokenExpired` state. Add a test for expired token behavior.

- [ ] **Step 4: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/KaikuWebSocket.kt \
  mobile/android/app/src/test/java/io/wolftown/kaiku/data/ws/KaikuWebSocketTest.kt
git commit -m "fix(client): check token expiry before WS reconnect, track connectivity collector (#16, #17)"
```

---

## Task 4: WebSocket auth migration — server dual-auth (#1, server side)

**Files:**
- Modify: `server/src/ws/events.rs:65`
- Modify: `server/src/ws/handlers.rs:36-96,420`

- [ ] **Step 1: Add Authenticate variant to ClientEvent**

At `server/src/ws/events.rs:65`, inside `pub enum ClientEvent`, add a new variant:

```rust
/// Post-connect authentication (replaces header-based auth for new clients).
Authenticate {
    /// JWT access token.
    token: String,
},
```

- [ ] **Step 2: Modify ws_handler to support dual auth**

At `server/src/ws/handlers.rs`, the current flow at line 87-96 calls `extract_token_from_protocol()` and returns 401 if no token found — *before* upgrading the WebSocket. Restructure to:

1. Try `extract_token_from_protocol()` first (header auth — existing path).
2. If header auth succeeds: upgrade with existing logic (backwards compatible, no behavior change).
3. If header auth fails (no header present): upgrade the WebSocket anyway, then enter a pre-auth message loop.

The pre-auth loop:

```rust
// In ws_handler, after the header auth check fails:
// Accept the WebSocket upgrade without authentication
let (response, ws_stream) = /* upgrade without requiring token */;

// Pre-auth loop: wait for Authenticate frame with 5s timeout
let user_id = match tokio::time::timeout(
    std::time::Duration::from_secs(5),
    wait_for_authenticate(&mut ws_stream, &state),
).await {
    Ok(Ok(uid)) => uid,
    Ok(Err(e)) => {
        // Invalid token — close with error
        let _ = ws_stream.close(Some(CloseFrame { code: CloseCode::Policy, reason: e.into() })).await;
        return;
    }
    Err(_) => {
        // Timeout — close connection
        let _ = ws_stream.close(Some(CloseFrame { code: CloseCode::Policy, reason: "Auth timeout".into() })).await;
        return;
    }
};

// Continue with the existing authenticated message loop using user_id
```

Create `wait_for_authenticate` as a helper:

```rust
async fn wait_for_authenticate(
    ws_stream: &mut WebSocketStream,
    state: &AppState,
) -> Result<Uuid, String> {
    while let Some(Ok(msg)) = ws_stream.next().await {
        if let Message::Text(text) = msg {
            if let Ok(ClientEvent::Authenticate { token }) = serde_json::from_str(&text) {
                // Validate JWT using the same logic as extract_token_from_protocol
                let user_id = validate_jwt_token(&token, &state.jwt_keys)
                    .map_err(|e| format!("Invalid token: {e}"))?;
                return Ok(user_id);
            }
            // Ignore non-Authenticate events during pre-auth
        }
    }
    Err("Connection closed before authentication".to_string())
}
```

- [ ] **Step 3: Handle Authenticate in dispatch (post-auth, ignore)**

At `handlers.rs:420`, in `handle_client_message`, add:

```rust
ClientEvent::Authenticate { .. } => {
    // Already authenticated — ignore duplicate
    Ok(())
}
```

- [ ] **Step 4: Build and verify server**

Run: `SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings && cargo test -p vc-server`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add server/src/ws/events.rs server/src/ws/handlers.rs
git commit -m "feat(ws): support post-connect Authenticate frame alongside header auth (#1)"
```

---

## Task 5: WebSocket auth migration — Android client (#1, client side)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ClientEvent.kt`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/KaikuWebSocket.kt:145`
- Modify: `mobile/android/app/src/test/java/io/wolftown/kaiku/data/ws/KaikuWebSocketTest.kt:176`

- [ ] **Step 1: Add Authenticate variant to Kotlin ClientEvent**

At `data/ws/ClientEvent.kt`, inside the `sealed class ClientEvent`, add:

```kotlin
@Serializable
@SerialName("authenticate")
data class Authenticate(val token: String) : ClientEvent()
```

This uses `@SerialName("authenticate")` matching the server's `#[serde(rename_all = "snake_case")]` convention. The existing sealed class serialization will produce `{"type":"authenticate","token":"..."}`.

- [ ] **Step 2: Remove token from header, send Authenticate frame**

At `KaikuWebSocket.kt:145`, remove `.addHeader("Sec-WebSocket-Protocol", "access_token.$token")` from the request builder.

In the `onOpen` callback of the WebSocket listener, send an Authenticate event as the first frame using the typed sealed class (not raw JSON):

```kotlin
override fun onOpen(webSocket: WebSocket, response: Response) {
    val authEvent = json.encodeToString(
        ClientEvent.serializer(),
        ClientEvent.Authenticate(token)
    )
    webSocket.send(authEvent)
    // ... existing onOpen logic
}
```

Capture `token` in the listener's closure (it's already available from `doConnect`).

- [ ] **Step 3: Update WebSocket tests**

At `KaikuWebSocketTest.kt:176`, update `connect uses Sec-WebSocket-Protocol header with token` to verify:
1. No `Sec-WebSocket-Protocol` header is sent.
2. First message after open is `{"type":"authenticate","token":"..."}`.

- [ ] **Step 4: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/ClientEvent.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/data/ws/KaikuWebSocket.kt \
  mobile/android/app/src/test/java/io/wolftown/kaiku/data/ws/KaikuWebSocketTest.kt
git commit -m "fix(client): move WS auth from header to post-connect Authenticate frame (#1)"
```

---

## Task 6: OIDC PKCE + state nonce (#3, Part A)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/auth/OidcHandler.kt:42,79`

- [ ] **Step 1: Generate and persist PKCE + state in launchOidcLogin**

At `OidcHandler.kt:42`, in `launchOidcLogin()`:

```kotlin
import java.security.MessageDigest
import java.security.SecureRandom
import android.util.Base64

// Generate code_verifier (43-128 chars, URL-safe base64)
val codeVerifier = ByteArray(32).also { SecureRandom().nextBytes(it) }
    .let { Base64.encodeToString(it, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING) }

// Compute code_challenge = BASE64URL(SHA256(code_verifier))
val codeChallenge = MessageDigest.getInstance("SHA-256")
    .digest(codeVerifier.toByteArray(Charsets.US_ASCII))
    .let { Base64.encodeToString(it, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING) }

// Generate state nonce
val state = ByteArray(32).also { SecureRandom().nextBytes(it) }
    .let { Base64.encodeToString(it, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING) }

// Persist to EncryptedSharedPreferences
tokenStorage.saveOidcPkceState(codeVerifier, state)

// Add to authorization URL
// &code_challenge=$codeChallenge&code_challenge_method=S256&state=$state
```

- [ ] **Step 2: Validate state and include code_verifier in handleCallback**

At `OidcHandler.kt:79`, in `handleCallback()`:

For the **primary token path** (lines 82-93, where `access_token` is in the URI):

```kotlin
// Before calling authRepository.completeOidcLogin:
val savedState = tokenStorage.getOidcState()
val uriState = uri.getQueryParameter("state")
if (savedState == null || uriState != savedState) {
    tokenStorage.clearOidcPkceState()
    return Result.failure(IllegalStateException("OIDC state mismatch — possible CSRF"))
}
tokenStorage.clearOidcPkceState()
// Then proceed with completeOidcLogin as before
```

For the **authorization code path** (lines 95-105, where `code` is in the URI):

```kotlin
val code = uri.getQueryParameter("code") ?: return Result.failure(...)
val uriState = uri.getQueryParameter("state") ?: return Result.failure(...)

val savedState = tokenStorage.getOidcState()
if (savedState == null || uriState != savedState) {
    tokenStorage.clearOidcPkceState()
    return Result.failure(IllegalStateException("OIDC state mismatch — possible CSRF"))
}

val codeVerifier = tokenStorage.getOidcCodeVerifier()
tokenStorage.clearOidcPkceState()

// Pass code_verifier to the code exchange
return authRepository.exchangeOidcCode(code, uriState, codeVerifier)
```

- [ ] **Step 2b: Update AuthRepository.exchangeOidcCode to accept codeVerifier**

At `AuthRepository.kt:162`, update the method signature:

```kotlin
suspend fun exchangeOidcCode(code: String, state: String, codeVerifier: String?): Result<User>
```

Pass `codeVerifier` through to the API call that performs the token exchange. The token exchange HTTP request body should include `code_verifier=$codeVerifier` when non-null.

- [ ] **Step 3: Add PKCE storage methods to TokenStorage**

Add `saveOidcPkceState(codeVerifier, state)`, `getOidcState()`, `getOidcCodeVerifier()`, `clearOidcPkceState()` methods using the existing `EncryptedSharedPreferences` instance.

- [ ] **Step 4: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/ui/auth/OidcHandler.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/TokenStorage.kt
git commit -m "fix(auth): add PKCE and state nonce to OIDC flow (#3)"
```

---

## Task 7: App Links intent filter (#3, Part B)

**Files:**
- Modify: `mobile/android/app/src/main/AndroidManifest.xml`

- [ ] **Step 1: Add App Links intent filter**

In `AndroidManifest.xml`, add a second intent filter for the `https://` scheme alongside the existing `kaiku://` filter:

```xml
<intent-filter android:autoVerify="true">
    <action android:name="android.intent.action.VIEW" />
    <category android:name="android.intent.category.DEFAULT" />
    <category android:name="android.intent.category.BROWSABLE" />
    <data android:scheme="https"
          android:host="kaiku.pmind.de"
          android:pathPrefix="/auth/callback" />
</intent-filter>
```

- [ ] **Step 2: Scope legacy BLUETOOTH permission (#27)**

In the same file, change:
```xml
<uses-permission android:name="android.permission.BLUETOOTH" />
```
To:
```xml
<uses-permission android:name="android.permission.BLUETOOTH"
    android:maxSdkVersion="30" />
```

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/AndroidManifest.xml
git commit -m "fix(auth): add App Links for OIDC callback, scope BLUETOOTH permission (#3, #27)"
```

---

## Task 8: Defer token persistence until getMe (#4)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/AuthRepository.kt`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/api/KaikuHttpClient.kt`

There are **5 auth flows** with the double-save pattern: `login` (line 31), `register` (line 69), `completeOidcLogin` (line 131), `exchangeOidcCode` (line 167), and `redeemQrToken` (line 207). All must be fixed.

- [ ] **Step 1: Add authenticatedGetMe to KaikuHttpClient**

The current `getMe()` reads the token from `TokenStorage` via the auth interceptor. Add an overload that accepts tokens directly:

```kotlin
suspend fun getMe(accessToken: String): User {
    return client.get("${baseUrl}/api/users/me") {
        header("Authorization", "Bearer $accessToken")
    }.body()
}
```

This bypasses the interceptor chain and uses the provided token directly.

- [ ] **Step 2: Refactor all 5 auth flows to defer persistence**

In `AuthRepository.kt`, for each flow, replace the double-save pattern:

```kotlin
// BEFORE (e.g., login at lines 31-46):
tokenStorage.saveTokens(response.accessToken, response.refreshToken, "")
val user = authApi.getMe()
tokenStorage.saveTokens(response.accessToken, response.refreshToken, user.id)

// AFTER:
val user = try {
    httpClient.getMe(response.accessToken)
} catch (e: Exception) {
    throw e  // No tokens persisted on failure
}
tokenStorage.saveTokens(response.accessToken, response.refreshToken, user.id)
```

Apply to all 5 flows: `login` (line 31), `register` (line 69), `completeOidcLogin` (line 131), `exchangeOidcCode` (line 167), `redeemQrToken` (line 207).

- [ ] **Step 3: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS — existing auth tests should still work since the external behavior is the same

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/repository/AuthRepository.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/data/api/KaikuHttpClient.kt
git commit -m "fix(auth): defer token persistence until getMe succeeds (#4)"
```

---

## Task 9: Final verification

- [ ] **Step 1: Run full test suite**

```bash
cd mobile/android && ./gradlew test
SQLX_OFFLINE=true cargo clippy -p vc-server -- -D warnings
cargo test -p vc-server
```
Expected: ALL PASS

- [ ] **Step 2: Self-review the branch diff**

```bash
git diff main...HEAD --stat
git log --oneline main..HEAD
```

Verify: no secrets, correct scope, proper error handling, all 9 issues addressed.
