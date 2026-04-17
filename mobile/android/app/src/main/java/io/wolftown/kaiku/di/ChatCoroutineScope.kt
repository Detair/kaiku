package io.wolftown.kaiku.di

import javax.inject.Qualifier

/**
 * Qualifier for the [kotlinx.coroutines.CoroutineScope] used by chat-layer
 * background work (e.g., [ChatRepository]'s WebSocket event collector).
 *
 * Tests inject a `TestScope` / `backgroundScope` via this qualifier so the
 * `TestCoroutineScheduler` drives all emissions synchronously.
 */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class ChatCoroutineScope
