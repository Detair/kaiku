package io.wolftown.kaiku.di

import android.os.SystemClock
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.ws.WallClock
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import javax.inject.Singleton

/**
 * App-lifetime scope and clock for the realtime connection manager. The
 * manager serialises its own state transitions on one actor coroutine, so a
 * shared dispatcher is fine.
 */
@Module
@InstallIn(SingletonComponent::class)
object RealtimeCoroutineScopeModule {
    @Provides
    @Singleton
    @RealtimeCoroutineScope
    fun provideRealtimeCoroutineScope(): CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.Default)

    @Provides
    @Singleton
    // Monotonic (keeps counting in deep sleep, immune to clock changes) for pong/watchdog timing.
    fun provideWallClock(): WallClock = WallClock(SystemClock::elapsedRealtime)
}
