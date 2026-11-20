package io.wolftown.kaiku.ui.auth.setup

import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.DeviceSuitabilityChecker
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.keydevice.KeyAnswers
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.ServerSilence
import io.wolftown.kaiku.data.keydevice.SetupDiscard
import io.wolftown.kaiku.testutil.SigFixtures
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import java.time.Instant

/** What the setup needs from the app around it, scripted. */
class FakeSetupEnvironment : SetupEnvironment {
    var pinned: String? = null
    /** When set, the pin follows the phone's state (as OriginPin does): the key's origin, else [pinned]. */
    var pinFrom: (() -> String?)? = null
    var signedInUser: String? = null
    val saved = mutableListOf<String>()
    /** Whether the app holds a key record right now (as the real environment asks the repository). */
    var hasRecord: () -> Boolean = { false }
    override fun pinnedOrigin() = pinFrom?.invoke() ?: pinned
    override fun signedInUserId() = signedInUser
    /** The app's server address as a test sets it; a setup never changes it. */
    var stored: String? = null
    override val deviceLabel = "Google Pixel 8"
    override val apiLevel = 35
}

/**
 * The one setup of a test phone, on the test's main dispatcher (as in the app, where its steps run
 * on the main thread). Its scope outlives every screen, like the app's.
 */
internal fun setupOwner(
    enroll: EnrollmentApi,
    repo: KeyDeviceRepository,
    signer: DeviceSigner,
    deviceSignIn: DeviceSignIn,
    probe: KeyDeviceProbe,
    env: SetupEnvironment = FakeSetupEnvironment(),
    suitability: DeviceSuitabilityChecker = object : DeviceSuitabilityChecker {
        override fun check(): UvDecision = UvPolicy.choose(30, true, true)
    },
    keyDispatcher: CoroutineDispatcher = Dispatchers.Unconfined,
    scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate),
    /** The phone's own clock. */
    clock: () -> Instant = Instant::now,
    /** The phone's running time. */
    boot: io.wolftown.kaiku.data.keydevice.BootClock = io.wolftown.kaiku.testutil.FakeBootClock(),
    /** The install id the app has now. */
    installId: () -> String = { SigFixtures.DEVICE_ID },
): SetupOwner = SetupOwner(
    enroll, suitability, signer, repo, installId, deviceSignIn, env, probe, KeyAnswers(repo, boot),
    SetupDiscard(repo, ServerSilence(repo, boot)), scope, keyDispatcher, clock, boot,
)
