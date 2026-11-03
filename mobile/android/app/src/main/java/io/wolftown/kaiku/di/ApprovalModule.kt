package io.wolftown.kaiku.di

import android.content.Context
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.approval.AndroidApprovalKeyStore
import io.wolftown.kaiku.approval.AndroidDeviceSuitabilityChecker
import io.wolftown.kaiku.approval.ApprovalKeyStore
import io.wolftown.kaiku.approval.BiometricUserVerifier
import io.wolftown.kaiku.approval.ChallengeVerifier
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.DeviceSuitabilityChecker
import io.wolftown.kaiku.approval.UserVerifier
import io.wolftown.kaiku.data.local.TokenStorage
import javax.inject.Singleton

/** The phone-as-key pieces: K_approve, the suitability check, the UV prompt and the signer. */
@Module
@InstallIn(SingletonComponent::class)
object ApprovalModule {

    @Provides
    @Singleton
    fun provideApprovalKeyStore(@ApplicationContext context: Context): ApprovalKeyStore =
        AndroidApprovalKeyStore(context)

    @Provides
    fun provideSuitabilityChecker(@ApplicationContext context: Context): DeviceSuitabilityChecker =
        AndroidDeviceSuitabilityChecker(context)

    @Provides
    @Singleton
    fun provideUserVerifier(keyStore: ApprovalKeyStore): UserVerifier = BiometricUserVerifier(keyStore)

    /** The pinned origin is the configured server (stored normalised as `https://host[:port]`). */
    @Provides
    fun provideChallengeVerifier(tokenStorage: TokenStorage): ChallengeVerifier =
        ChallengeVerifier(
            pinnedOrigin = { tokenStorage.getServerUrl() },
            nowSeconds = { System.currentTimeMillis() / 1000 },
        )

    @Provides
    fun provideDeviceSigner(
        keyStore: ApprovalKeyStore,
        verifier: UserVerifier,
        checks: ChallengeVerifier,
    ): DeviceSigner = DeviceSigner(keyStore, verifier, checks)
}
