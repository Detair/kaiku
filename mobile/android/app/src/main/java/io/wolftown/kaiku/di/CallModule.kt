package io.wolftown.kaiku.di

import dagger.Binds
import dagger.Module
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.call.CallDirectory
import io.wolftown.kaiku.data.call.CallNotifier
import io.wolftown.kaiku.data.call.DmCallDirectory
import io.wolftown.kaiku.push.SocketCallNotifier

@Module
@InstallIn(SingletonComponent::class)
abstract class CallModule {

    @Binds
    abstract fun bindCallNotifier(impl: SocketCallNotifier): CallNotifier

    @Binds
    abstract fun bindCallDirectory(impl: DmCallDirectory): CallDirectory
}
