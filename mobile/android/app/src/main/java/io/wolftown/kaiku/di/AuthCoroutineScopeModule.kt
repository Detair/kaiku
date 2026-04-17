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
 * Provides the application-scoped [CoroutineScope] used by auth-layer
 * background collectors (e.g., `AuthState.isLoggedIn.stateIn(...)`).
 *
 * Kept separate from [AuthModule] because that module is `abstract` for
 * `@Binds` entries; Hilt requires `@Provides` to live in `object` or
 * non-abstract modules.
 *
 * `SupervisorJob` ensures a child coroutine's failure does not cascade to
 * siblings. `Dispatchers.Default` matches the prior (inline) behavior —
 * the scope's only work is a cheap `.map` transform on an in-memory flow,
 * not I/O.
 */
@Module
@InstallIn(SingletonComponent::class)
object AuthCoroutineScopeModule {
    @Provides
    @Singleton
    @AuthCoroutineScope
    fun provideAuthCoroutineScope(): CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.Default)
}
