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
import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.data.keydevice.KeyDevicePolicy
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceStore
import io.wolftown.kaiku.data.keydevice.PrefsKeyDeviceStore
import io.wolftown.kaiku.data.local.KeystoreValueCipher
import io.wolftown.kaiku.data.local.KeystoreWrappedPreferences
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.push.DeviceIdStore
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
            pinnedOrigin = { tokenStorage.getServerUrl()?.let(KaikuPayloads::canonicalOrigin) },
            nowSeconds = { System.currentTimeMillis() / 1000 },
        )

    /** Keystore-wrapped and separate from the session file: it outlives sign-outs and lapses. */
    @Provides
    @Singleton
    fun provideKeyDeviceStore(@ApplicationContext context: Context): KeyDeviceStore =
        PrefsKeyDeviceStore(
            KeystoreWrappedPreferences(
                context.getSharedPreferences("kaiku_key_device", Context.MODE_PRIVATE),
                KeystoreValueCipher(context),
            ),
        )

    @Provides
    fun provideKeyDevicePolicy(repo: KeyDeviceRepository): KeyDevicePolicy = repo

    /** The C26 device id is the C19 push device id. */
    @Provides
    fun provideDeviceIdProvider(store: DeviceIdStore): DeviceIdProvider = store

    @Provides
    fun provideSetupEnvironment(impl: io.wolftown.kaiku.ui.auth.setup.AndroidSetupEnvironment):
        io.wolftown.kaiku.ui.auth.setup.SetupEnvironment = impl

    @Provides
    fun provideDeviceSigner(
        keyStore: ApprovalKeyStore,
        verifier: UserVerifier,
        checks: ChallengeVerifier,
    ): DeviceSigner = DeviceSigner(keyStore, verifier, checks)
}
