package io.wolftown.kaiku.di

import javax.inject.Qualifier

/**
 * Qualifier for the [kotlinx.coroutines.CoroutineScope] used by auth-layer
 * background work (e.g., `AuthState.isLoggedIn` / `currentUserId` stateIn
 * collection).
 *
 * Tests inject a `TestScope` / `backgroundScope` via this qualifier so the
 * `TestCoroutineScheduler` drives all emissions synchronously.
 */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class AuthCoroutineScope
