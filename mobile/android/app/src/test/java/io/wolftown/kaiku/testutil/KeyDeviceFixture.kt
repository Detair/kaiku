package io.wolftown.kaiku.testutil

import io.wolftown.kaiku.approval.ChallengeVerifier
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.FakeUserVerifier
import io.wolftown.kaiku.approval.SoftwareApprovalKeyStore
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPolicy
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import kotlinx.coroutines.CoroutineScope

/** An active key device for the C26 fixture user/device, with a software key and a scripted UV. */
class KeyDeviceFixture(scope: CoroutineScope, uv: UvResult? = null, active: Boolean = true) {
    val keyStore = SoftwareApprovalKeyStore().apply {
        generate(ByteArray(32), UvPolicy.choose(30, true, true) as UvDecision.Use)
    }
    val authState = AuthState(scope)
    val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), keyStore, authState, scope)
    val verifier = FakeUserVerifier(keyStore, uv)
    val checks = ChallengeVerifier({ SigFixtures.ORIGIN }, { SigFixtures.EXPIRES_AT - 30 })
    val signer = DeviceSigner(keyStore, verifier, checks)

    init {
        authState.setLoggedIn(SigFixtures.USER_ID)
        if (active) {
            repo.saveClaimed(LocalKeyDevice("kd1", SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null, "Alice"))
            repo.markActive(SigFixtures.USER_ID, "kd1")
        }
    }
}
