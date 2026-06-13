package io.wolftown.kaiku.di

import dagger.Binds
import dagger.Module
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.api.DmApi
import io.wolftown.kaiku.data.api.DmApiImpl
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
abstract class DmModule {

    @Binds
    @Singleton
    abstract fun bindDmApi(impl: DmApiImpl): DmApi
}
