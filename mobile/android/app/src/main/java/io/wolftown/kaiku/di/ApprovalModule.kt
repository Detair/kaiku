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
import io.wolftown.kaiku.data.auth.DeviceStepUp
import io.wolftown.kaiku.data.auth.DeviceStepUpProvider
import io.wolftown.kaiku.data.auth.OriginPin
import io.wolftown.kaiku.data.auth.PasskeyStepUp
import io.wolftown.kaiku.data.auth.StepUpProvider
import io.wolftown.kaiku.data.keydevice.KeyDevicePolicy
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceStore
import io.wolftown.kaiku.data.keydevice.PrefsKeyDeviceStore
import io.wolftown.kaiku.data.local.KeystoreValueCipher
import io.wolftown.kaiku.data.local.KeystoreWrappedPreferences
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

    /** The pinned origin is the key's server, else the configured one (B.1, [OriginPin.signingOrigin]). */
    @Provides
    fun provideChallengeVerifier(originPin: OriginPin): ChallengeVerifier =
        ChallengeVerifier(
            pinnedOrigin = originPin::signingOrigin,
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

    /**
     * The key device's step-up for LP's seam (`@DeviceStepUp`, LP's optional
     * binding in `PasskeyModule`): LP's passkey Add gets its `passkey.add` grant
     * from the key on the key phone (DC1) and from a passkey elsewhere.
     */
    @Provides
    @DeviceStepUp
    fun provideDeviceStepUpProvider(
        stepUp: io.wolftown.kaiku.approval.DeviceStepUp,
        passkeys: PasskeyStepUp,
    ): StepUpProvider = DeviceStepUpProvider(stepUp, fallback = passkeys)

    /** L2's screens: the key on the key phone, else a passkey, for exactly the action and target. */
    @Provides
    fun provideAccountStepUp(impl: io.wolftown.kaiku.data.auth.KeyOrPasskeyStepUp): io.wolftown.kaiku.data.auth.AccountStepUp = impl

    @Provides
    fun provideDeviceSigner(
        keyStore: ApprovalKeyStore,
        verifier: UserVerifier,
        checks: ChallengeVerifier,
        invalidation: io.wolftown.kaiku.data.keydevice.KeyInvalidation,
    ): DeviceSigner = DeviceSigner(keyStore, verifier, checks, invalidation::handle)
}
