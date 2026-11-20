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
import io.wolftown.kaiku.testutil.SigFixtures
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

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
    var stored: String? = null
    override fun adoptServer(origin: String): Boolean {
        if (hasRecord() || pinnedOrigin() != null) return false
        saved += origin
        stored = origin
        return true
    }
    override fun savedServer() = stored
    override fun restoreServer(url: String?) { stored = url }
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
): SetupOwner = SetupOwner(
    enroll, suitability, signer, repo, { SigFixtures.DEVICE_ID }, deviceSignIn, env, probe, KeyAnswers(repo), scope, keyDispatcher,
)
