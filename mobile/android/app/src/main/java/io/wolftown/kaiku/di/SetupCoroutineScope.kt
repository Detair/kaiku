package io.wolftown.kaiku.di

import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import javax.inject.Qualifier
import javax.inject.Singleton

/** The scope of the one setup of this phone: as long as the app, its steps on the main thread. */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class SetupCoroutineScope

@Module
@InstallIn(SingletonComponent::class)
object SetupCoroutineScopeModule {
    @Provides
    @Singleton
    @SetupCoroutineScope
    fun provideSetupScope(): CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
}
