package io.wolftown.kaiku.di

import dagger.Binds
import dagger.Module
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.api.MessageApi
import io.wolftown.kaiku.data.api.MessageApiImpl
import io.wolftown.kaiku.data.api.UnreadApi
import io.wolftown.kaiku.data.api.UnreadApiImpl
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
abstract class ChatModule {

    @Binds
    @Singleton
    abstract fun bindMessageApi(impl: MessageApiImpl): MessageApi

    @Binds
    @Singleton
    abstract fun bindUnreadApi(impl: UnreadApiImpl): UnreadApi
}
