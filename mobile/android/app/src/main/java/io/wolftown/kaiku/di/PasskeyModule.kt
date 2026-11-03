package io.wolftown.kaiku.di

import dagger.Binds
import dagger.BindsOptionalOf
import dagger.Module
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.api.PasskeyApi
import io.wolftown.kaiku.data.api.PasskeyApiImpl
import io.wolftown.kaiku.data.auth.CredentialManagerPasskeys
import io.wolftown.kaiku.data.auth.DeviceStepUp
import io.wolftown.kaiku.data.auth.PasskeyCredentials
import io.wolftown.kaiku.data.auth.StepUpProvider

@Module
@InstallIn(SingletonComponent::class)
abstract class PasskeyModule {
    @Binds
    abstract fun bindPasskeyApi(impl: PasskeyApiImpl): PasskeyApi

    @Binds
    abstract fun bindPasskeyCredentials(impl: CredentialManagerPasskeys): PasskeyCredentials

    /** The key device's step-up (L2 binds it; absent until then). */
    @BindsOptionalOf
    @DeviceStepUp
    abstract fun optionalDeviceStepUp(): StepUpProvider
}
