package io.wolftown.kaiku.di

import javax.inject.Qualifier

/**
 * Qualifier for the [kotlinx.coroutines.CoroutineScope] used by voice-layer
 * background work (e.g., [WebRtcManager.voiceIceConnected] stateIn collection).
 *
 * Tests inject a `TestScope` / `backgroundScope` via this qualifier so the
 * `TestCoroutineScheduler` drives all emissions synchronously.
 */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class VoiceCoroutineScope
