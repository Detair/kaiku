package io.wolftown.kaiku.di

import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import javax.inject.Singleton

/**
 * Provides the application-scoped [CoroutineScope] used by voice-layer
 * background collectors (e.g., `WebRtcManager.voiceIceConnected.stateIn(...)`).
 *
 * Kept separate from [VoiceModule] because that module is `abstract` for
 * `@Binds` entries; Hilt requires `@Provides` to live in `object` or
 * non-abstract modules.
 *
 * `SupervisorJob` ensures a child coroutine's failure does not cascade to
 * siblings. `Dispatchers.IO` matches the prior (inline) behavior.
 */
@Module
@InstallIn(SingletonComponent::class)
object VoiceCoroutineScopeModule {
    @Provides
    @Singleton
    @VoiceCoroutineScope
    fun provideVoiceCoroutineScope(): CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.IO)
}
