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
 * Provides the application-scoped [CoroutineScope] used by chat-layer
 * background collectors (e.g., `ChatRepository`'s WebSocket event collector).
 *
 * Kept separate from [ChatModule] because that module is `abstract` for
 * `@Binds` entries; Hilt requires `@Provides` to live in `object` or
 * non-abstract modules.
 *
 * `SupervisorJob` ensures a child coroutine's failure does not cascade to
 * siblings. `Dispatchers.IO` matches the prior (inline) behavior — the
 * WebSocket event collector benefits from an I/O dispatcher.
 */
@Module
@InstallIn(SingletonComponent::class)
object ChatCoroutineScopeModule {
    @Provides
    @Singleton
    @ChatCoroutineScope
    fun provideChatCoroutineScope(): CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.IO)
}
