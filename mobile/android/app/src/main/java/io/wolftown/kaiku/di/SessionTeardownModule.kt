package io.wolftown.kaiku.di

import dagger.Binds
import dagger.Module
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import dagger.multibindings.IntoSet
import io.wolftown.kaiku.data.session.CacheSessionTeardown
import io.wolftown.kaiku.data.session.RealtimeSessionTeardown
import io.wolftown.kaiku.data.session.SessionTeardown
import io.wolftown.kaiku.data.session.VoiceSessionTeardown

/**
 * The steps [io.wolftown.kaiku.data.session.SessionTeardownRunner] runs when a
 * session ends, ordered by [SessionTeardown.order]. Order 0 is reserved for
 * push unregistration (push plan Task 7), which must stay first.
 */
@Module
@InstallIn(SingletonComponent::class)
abstract class SessionTeardownModule {

    @Binds
    @IntoSet
    abstract fun bindVoiceTeardown(impl: VoiceSessionTeardown): SessionTeardown

    @Binds
    @IntoSet
    abstract fun bindRealtimeTeardown(impl: RealtimeSessionTeardown): SessionTeardown

    @Binds
    @IntoSet
    abstract fun bindCacheTeardown(impl: CacheSessionTeardown): SessionTeardown
}
