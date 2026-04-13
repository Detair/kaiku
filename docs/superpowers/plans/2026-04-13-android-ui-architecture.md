# Android UI & Architecture Fixes — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix navigation event drops, channel name display, auto-scroll behavior, CoroutineScope hygiene, accessibility, and ViewModel lifecycle patterns.

**Architecture:** Seven independent fixes across UI and architecture layers. Most are small (1-5 line changes). The channel name resolution (#7) and navigation Channel pattern (#8) are the most involved, requiring ViewModel constructor changes.

**Tech Stack:** Kotlin, Jetpack Compose, Hilt, Coroutines, Channel/Flow

**Spec:** `docs/superpowers/specs/2026-04-13-mobile-implementation-fixes-design.md` — Sub-Project 3

**Branch:** `fix/android-ui-architecture`

---

## File Map

| File | Responsibility |
|------|---------------|
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/home/HomeViewModel.kt` | SharedFlow → Channel for nav events |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/auth/LoginViewModel.kt` | SharedFlow → Channel for OIDC callback |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/KaikuNavGraph.kt` | Remove channelName param from routes |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/TextChannelViewModel.kt` | Add channel name resolution via GuildRepository |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/TextChannelScreen.kt` | Auto-scroll guard, read channelName from ViewModel |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceViewModel.kt` | Add channel name resolution |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceChannelScreen.kt` | Read channelName from ViewModel |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/AuthState.kt` | Named scope, Closeable |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/home/GuildSidebar.kt` | Button semantics |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/MessageItem.kt` | remember(formatTimestamp) |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/settings/SettingsViewModel.kt` | Channel-based logout event |
| `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/settings/SettingsScreen.kt` | Collect logout event |

---

## Task 1: Navigation SharedFlow → Channel (#8)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/home/HomeViewModel.kt:50`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/auth/LoginViewModel.kt:39`

- [ ] **Step 1: Replace SharedFlow with Channel in HomeViewModel**

At `HomeViewModel.kt:50`, change:

```kotlin
private val _navigateToChannel = MutableSharedFlow<ChannelNavEvent>(extraBufferCapacity = 1)
val navigateToChannel: SharedFlow<ChannelNavEvent> = _navigateToChannel.asSharedFlow()
```

To:

```kotlin
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.receiveAsFlow

private val _navigateToChannel = Channel<ChannelNavEvent>(Channel.BUFFERED)
val navigateToChannel = _navigateToChannel.receiveAsFlow()
```

Change all `_navigateToChannel.tryEmit(...)` calls to `_navigateToChannel.trySend(...)`.

- [ ] **Step 2: Apply same pattern in LoginViewModel**

At `LoginViewModel.kt:39`, apply the same `SharedFlow → Channel` change to `_oidcCallbackUri`.

- [ ] **Step 3: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS — HomeViewModelTest and LoginViewModelTest should pass since the external API is similar

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/ui/home/HomeViewModel.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/ui/auth/LoginViewModel.kt
git commit -m "fix(client): replace navigation SharedFlow with Channel to prevent event drops (#8)"
```

---

## Task 2: Channel name resolution (#7)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/TextChannelViewModel.kt:18`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/TextChannelScreen.kt`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceViewModel.kt`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceChannelScreen.kt`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/KaikuNavGraph.kt:116`

- [ ] **Step 1: Add GuildRepository injection and channelName flow to TextChannelViewModel**

At `TextChannelViewModel.kt:18`, add `GuildRepository` to the constructor:

```kotlin
@HiltViewModel
class TextChannelViewModel @Inject constructor(
    savedStateHandle: SavedStateHandle,
    private val chatRepository: ChatRepository,
    private val guildRepository: GuildRepository
) : ViewModel() {
```

Add a `channelName` StateFlow:

```kotlin
val channelName: StateFlow<String> = guildRepository.channels
    .map { channels -> channels.find { it.id == channelId }?.name ?: channelId }
    .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5000), channelId)
```

- [ ] **Step 2: Update TextChannelScreen to use ViewModel's channelName**

In `TextChannelScreen.kt`, remove the `channelName` parameter from the composable function signature. Instead, collect from the ViewModel:

```kotlin
val channelName by viewModel.channelName.collectAsState()
```

Use this in the TopAppBar title: `Text("#$channelName")`

- [ ] **Step 3: Apply same pattern to VoiceViewModel and VoiceChannelScreen**

Add `GuildRepository` injection and `channelName: StateFlow<String>` to `VoiceViewModel`. Update `VoiceChannelScreen` to read from the ViewModel.

- [ ] **Step 4: Remove channelName parameter from KaikuNavGraph**

At `KaikuNavGraph.kt:116`, remove `channelName = channelId` from both the `TextChannelScreen` and `VoiceChannelScreen` composable calls.

- [ ] **Step 5: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS — TextChannelViewModelTest may need updating to provide a mock GuildRepository

- [ ] **Step 6: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/TextChannelViewModel.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/TextChannelScreen.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceViewModel.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/ui/voice/VoiceChannelScreen.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/ui/KaikuNavGraph.kt
git commit -m "fix(client): resolve channel name from GuildRepository instead of displaying UUID (#7)"
```

---

## Task 3: Auto-scroll guard (#9)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/TextChannelScreen.kt:41`

- [ ] **Step 1: Replace unconditional scroll with guarded version**

At `TextChannelScreen.kt:41`, replace:

```kotlin
LaunchedEffect(messages.size) {
    if (messages.isNotEmpty()) {
        listState.animateScrollToItem(messages.size - 1)
    }
}
```

With:

```kotlin
var lastMessageId by remember { mutableStateOf<String?>(null) }

LaunchedEffect(messages.lastOrNull()?.id) {
    val newLastId = messages.lastOrNull()?.id
    if (newLastId != null && newLastId != lastMessageId) {
        lastMessageId = newLastId
        val lastVisible = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
        if (lastVisible >= messages.size - 4) {
            listState.animateScrollToItem(messages.size - 1)
        }
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/TextChannelScreen.kt
git commit -m "fix(client): guard auto-scroll to only fire on new messages when near bottom (#9)"
```

---

## Task 4: AuthState named scope (#20)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/AuthState.kt:25`

- [ ] **Step 1: Replace anonymous scopes with named field**

At `AuthState.kt:25`, replace:

```kotlin
val isLoggedIn: StateFlow<Boolean> = _session.map { ... }
    .stateIn(CoroutineScope(Dispatchers.Default), SharingStarted.Eagerly, false)

val currentUserId: StateFlow<String?> = _session.map { ... }
    .stateIn(CoroutineScope(Dispatchers.Default), SharingStarted.Eagerly, null)
```

With:

```kotlin
import java.io.Closeable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel

// Add Closeable to class declaration
class AuthState @Inject constructor() : Closeable {

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    val isLoggedIn: StateFlow<Boolean> = _session.map { ... }
        .stateIn(scope, SharingStarted.Eagerly, false)

    val currentUserId: StateFlow<String?> = _session.map { ... }
        .stateIn(scope, SharingStarted.Eagerly, null)

    override fun close() {
        scope.cancel()
    }
}
```

- [ ] **Step 2: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/data/local/AuthState.kt
git commit -m "fix(client): replace anonymous CoroutineScopes in AuthState with named cancellable scope (#20)"
```

---

## Task 5: Small fixes — GuildIcon semantics (#24), formatTimestamp remember (#25)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/home/GuildSidebar.kt:73`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/MessageItem.kt:90`

- [ ] **Step 1: Add button semantics to GuildIcon**

At `GuildSidebar.kt:73`, on the `Box` modifier chain for guild icons, add:

```kotlin
.semantics { role = Role.Button }
.clickable(
    onClick = onClick,
    onClickLabel = "Select ${guild.name}"
)
```

Import: `import androidx.compose.ui.semantics.Role`

- [ ] **Step 2: Wrap formatTimestamp in remember**

At `MessageItem.kt:90`, change:

```kotlin
Text(formatTimestamp(message.createdAt))
```

To:

```kotlin
val formattedTime = remember(message.createdAt) { formatTimestamp(message.createdAt) }
Text(formattedTime)
```

- [ ] **Step 3: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/ui/home/GuildSidebar.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/ui/channel/MessageItem.kt
git commit -m "fix(client): add GuildIcon button semantics and memoize formatTimestamp (#24, #25)"
```

---

## Task 6: SettingsViewModel logout event (#26)

**Files:**
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/settings/SettingsViewModel.kt:65`
- Modify: `mobile/android/app/src/main/java/io/wolftown/kaiku/ui/settings/SettingsScreen.kt:129`

- [ ] **Step 1: Replace UI callback with Channel event in SettingsViewModel**

At `SettingsViewModel.kt:65`, replace:

```kotlin
fun logout(onLogoutComplete: () -> Unit) {
    viewModelScope.launch {
        try { authRepository.logout() } catch (...) { }
        onLogoutComplete()
    }
}
```

With:

```kotlin
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.receiveAsFlow

private val _logoutComplete = Channel<Unit>(Channel.CONFLATED)
val logoutComplete = _logoutComplete.receiveAsFlow()

fun logout() {
    viewModelScope.launch {
        try {
            authRepository.logout()
        } catch (e: Exception) {
            logger.log(Level.WARNING, "Logout failed", e)
        }
        _logoutComplete.send(Unit)
    }
}
```

- [ ] **Step 2: Update SettingsScreen to collect logout event**

At `SettingsScreen.kt:129`, replace the direct callback:

```kotlin
// Remove: viewModel.logout { onLogout() }
// Add:
viewModel.logout()
```

Add a `LaunchedEffect` to collect the event:

```kotlin
LaunchedEffect(Unit) {
    viewModel.logoutComplete.collect { onLogout() }
}
```

- [ ] **Step 3: Build and verify**

Run: `cd mobile/android && ./gradlew test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add mobile/android/app/src/main/java/io/wolftown/kaiku/ui/settings/SettingsViewModel.kt \
  mobile/android/app/src/main/java/io/wolftown/kaiku/ui/settings/SettingsScreen.kt
git commit -m "fix(client): replace logout UI callback with Channel event for lifecycle safety (#26)"
```

---

## Task 7: Final verification

- [ ] **Step 1: Run full test suite**

```bash
cd mobile/android && ./gradlew test
```
Expected: ALL PASS

- [ ] **Step 2: Self-review the branch diff**

```bash
git diff main...HEAD --stat
git log --oneline main..HEAD
```

Verify: 7 issues addressed, no regressions.
