package io.wolftown.kaiku.ui.settings.security

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.auth.AccountStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.auth.KeyDeviceSignOut
import io.wolftown.kaiku.domain.model.AuthenticatorRef
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.DmParticipant
import io.wolftown.kaiku.domain.model.KeyDevice
import io.wolftown.kaiku.domain.model.KeyDeviceStatus
import io.wolftown.kaiku.domain.model.SecuritySession
import io.wolftown.kaiku.domain.model.SessionItem
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.FakeSecurityApi
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class SecuritySettingsViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val security = FakeSecurityApi()
    private val approvals = FakeApprovalApi()
    private val stepUp: AccountStepUp = mockk()
    private val signOut: KeyDeviceSignOut = mockk(relaxed = true)
    private val other = KeyDevice("kd2", label = "Old phone", state = KeyDeviceStatus.ACTIVE)
    private val mine = KeyDevice("kd1", label = "Pixel 8", state = KeyDeviceStatus.ACTIVE, isCurrent = true)
    private var dms: List<DmConversation> = emptyList()

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun granted(action: StepUpAction, target: String) {
        coEvery { stepUp.stepUp(activity, action, target) } returns
            StepUpOutcome.Granted(StepUpGrant(action.wire, target, "device", "t"))
    }

    private fun TestScope.vm(): SecuritySettingsViewModel {
        val f = KeyDeviceFixture(backgroundScope)
        approvals.devices = listOf(mine, other)
        return SecuritySettingsViewModel(security, approvals, stepUp, f.repo, signOut, { dms }, { SigFixtures.USER_ID })
            .also { it.load() }
    }

    @Test fun `probation session renders read only`() = runTest {
        security.me = security.me.copy(session = SecuritySession("probation", listOf("qr")))
        val v = vm()
        assertTrue(v.ui.value.readOnly)
        v.removeKeyDevice(activity, other)
        coVerify(exactly = 0) { stepUp.stepUp(any(), any(), any()) }
        assertEquals(R.string.security_probation_read_only, v.ui.value.message)
    }

    @Test fun `remove key device steps up for that device`() = runTest {
        granted(StepUpAction.KEY_DEVICE_REMOVE, "kd2")
        val v = vm()
        assertFalse(v.ui.value.readOnly)
        assertEquals(listOf("kd1", "kd2"), v.ui.value.keyDevices.map { it.id })
        v.removeKeyDevice(activity, other)
        coVerify { stepUp.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "kd2") }
        assertTrue("removeKeyDevice:kd2" in approvals.calls)
    }

    @Test fun `last strong authenticator shows add another first`() = runTest {
        granted(StepUpAction.KEY_DEVICE_REMOVE, "kd2")
        val v = vm()
        approvals.failWith = apiError(409, "LAST_STRONG_AUTHENTICATOR")
        v.removeKeyDevice(activity, other)
        assertEquals(R.string.security_last_strong, v.ui.value.message)
        assertFalse(v.ui.value.offerSignOutOthers)
    }

    @Test fun `authenticator change offers sign out everywhere else`() = runTest {
        granted(StepUpAction.KEY_DEVICE_REMOVE, "kd2")
        granted(StepUpAction.SESSIONS_END, "others")
        val v = vm()
        v.removeKeyDevice(activity, other)
        assertTrue(v.ui.value.offerSignOutOthers)
        v.confirmSignOutOthers(activity)
        assertTrue("endOtherSessions" in security.calls)
        assertFalse(v.ui.value.offerSignOutOthers)
    }

    @Test fun `end other sessions uses target others`() = runTest {
        granted(StepUpAction.SESSIONS_END, "others")
        val v = vm()
        v.endOtherSessions(activity)
        coVerify { stepUp.stepUp(activity, StepUpAction.SESSIONS_END, "others") }
        assertTrue("endOtherSessions" in security.calls)
    }

    @Test fun `ending a session steps up for its id and a key device session removes the device`() = runTest {
        granted(StepUpAction.SESSIONS_END, "s1")
        granted(StepUpAction.KEY_DEVICE_REMOVE, "kd2")
        val v = vm()
        v.endSession(activity, SessionItem("s1", device = "Firefox"))
        assertTrue("endSession:s1" in security.calls)
        v.endSession(activity, SessionItem("s2", authenticator = AuthenticatorRef("kd2", "key_device", "Old phone")))
        assertTrue("removeKeyDevice:kd2" in approvals.calls)
        assertFalse("endSession:s2" in security.calls)
    }

    @Test fun `recovery codes are shown once`() = runTest {
        granted(StepUpAction.RECOVERY_CODES_CREATE, "")
        val v = vm()
        v.createRecoveryCodes(activity)
        assertEquals(listOf("AAAA-BBBB-CCCC-D", "EEEE-FFFF-GGGG-H"), v.ui.value.newRecoveryCodes)
        assertFalse(v.ui.value.toString().contains("AAAA-BBBB"))
        v.acknowledgeRecoveryCodes()
        assertNull(v.ui.value.newRecoveryCodes)
    }

    @Test fun `recovery contact picker excludes the system account`() = runTest {
        dms = listOf(
            DmConversation("c1", participants = listOf(
                DmParticipant("kaiku", "kaiku", "Kaiku", isSystem = true),
                DmParticipant(SigFixtures.USER_ID, "alice", "Alice"),
            ), isSystem = true, readOnly = true),
            DmConversation("c2", participants = listOf(
                DmParticipant("u-oma", "oma", "Oma"),
                DmParticipant(SigFixtures.USER_ID, "alice", "Alice"),
            )),
        )
        granted(StepUpAction.RECOVERY_CONTACT_SET, "u-oma")
        val v = vm()
        val candidates = v.contactCandidates()
        assertEquals(listOf("u-oma"), candidates.map { it.userId })
        v.setRecoveryContact(activity, candidates.single())
        assertTrue("setRecoveryContact:u-oma" in security.calls)
    }

    @Test fun `an admin as recovery contact is refused with its text`() = runTest {
        granted(StepUpAction.RECOVERY_CONTACT_SET, "u-admin")
        val v = vm()
        security.failure = apiError(400, "RECOVERY_CONTACT_INVALID")
        v.setRecoveryContact(activity, ContactCandidate("u-admin", "admin", "Admin"))
        assertEquals(R.string.security_contact_invalid, v.ui.value.message)
    }

    @Test fun `a passkey session on a phone without a key changes settings with the passkey`() = runTest {
        val passkeys = io.wolftown.kaiku.testutil.RecordingPasskeyStepUp()
        approvals.devices = listOf(other)
        val v = SecuritySettingsViewModel(
            security, approvals, io.wolftown.kaiku.testutil.nonKeyPhoneStepUp(backgroundScope, passkeys),
            KeyDeviceFixture(backgroundScope).repo, signOut, { dms }, { SigFixtures.USER_ID },
        ).also { it.load() }
        v.removeKeyDevice(activity, other)
        assertEquals(listOf("key_device.remove:kd2"), passkeys.asked)
        assertTrue("removeKeyDevice:kd2" in approvals.calls)
    }

    @Test fun `a second tap while a change runs starts no second step-up`() = runTest {
        val gate = kotlinx.coroutines.CompletableDeferred<StepUpOutcome>()
        coEvery { stepUp.stepUp(activity, StepUpAction.KEY_DEVICE_REMOVE, "kd2") } coAnswers { gate.await() }
        val v = vm()
        v.removeKeyDevice(activity, other)
        v.removeKeyDevice(activity, other)
        v.endOtherSessions(activity)
        coVerify(exactly = 1) { stepUp.stepUp(any(), any(), any()) }
        gate.complete(StepUpOutcome.Cancelled)
        assertFalse(v.ui.value.working)
    }

    @Test fun `remove this phone is refused in a probation session and failures keep the app running`() = runTest {
        security.me = security.me.copy(session = SecuritySession("probation", listOf("qr")))
        val v = vm()
        v.signOutAndRemovePhone(activity)
        coVerify(exactly = 0) { signOut.signOutAndRemovePhone(any()) }
        assertEquals(R.string.security_probation_read_only, v.ui.value.message)

        security.me = security.me.copy(session = SecuritySession("full", listOf("device_key")))
        v.load()
        coEvery { signOut.signOutAndRemovePhone(activity) } throws java.io.IOException("offline")
        v.signOutAndRemovePhone(activity)
        assertEquals(R.string.security_network_error, v.ui.value.message)
        coEvery { signOut.signOutAndRemovePhone(activity) } returns StepUpOutcome.Failed("LAST_STRONG_AUTHENTICATOR")
        v.signOutAndRemovePhone(activity)
        assertEquals(R.string.security_last_strong, v.ui.value.message)
        assertFalse(v.ui.value.signedOut)
    }
}
