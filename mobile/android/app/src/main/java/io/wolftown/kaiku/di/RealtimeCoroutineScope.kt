package io.wolftown.kaiku.di

import javax.inject.Qualifier

/**
 * Qualifier for the [kotlinx.coroutines.CoroutineScope] that runs
 * [io.wolftown.kaiku.data.ws.RealtimeConnectionManager]. Tests inject a
 * `TestScope.backgroundScope` so virtual time drives every timer.
 */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class RealtimeCoroutineScope
