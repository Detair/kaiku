package io.wolftown.kaiku.ui.admin

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.auth.AccountStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.data.api.AdminApi
import io.wolftown.kaiku.domain.model.AdminUserDetail
import io.wolftown.kaiku.domain.model.AdminUserSummary
import io.wolftown.kaiku.domain.model.AuthenticatorRef
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class AdminUserViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val admin: AdminApi = mockk(relaxed = true)
    private val stepUp: AccountStepUp = mockk()
    private val detail = AdminUserDetail("u2", "oma", "Oma", mayApprove = false,
        keyDevices = listOf(AuthenticatorRef("kd1", "key_device", "Pixel 6")))

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun grant(action: StepUpAction, target: String) {
        coEvery { stepUp.stepUp(activity, action, target) } returns StepUpOutcome.Granted(StepUpGrant(action.wire, target, "device", "t"))
    }

    @Test fun `admin mode elevates after a device step-up`() = runTest {
        grant(StepUpAction.ADMIN_ELEVATE, "")
        coEvery { admin.status() } returns true
        coEvery { admin.users(null) } returns listOf(AdminUserSummary("u2", "oma", "Oma"))
        val v = AdminUsersViewModel(admin, stepUp)
        v.enter(activity)
        coVerify(ordering = io.mockk.Ordering.ORDERED) {
            stepUp.stepUp(activity, StepUpAction.ADMIN_ELEVATE, "")
            admin.elevate(null)
            admin.users(null)
        }
        assertTrue(v.ui.value.elevated)
        assertEquals(listOf("oma"), v.ui.value.users.map { it.username })
    }

    @Test fun `new account sends no password`() = runTest {
        grant(StepUpAction.ADMIN_ELEVATE, "")
        coEvery { admin.createUser("oma", "Oma", null) } returns AdminUserSummary("u2", "oma", "Oma")
        val v = AdminUsersViewModel(admin, stepUp)
        v.createUser("oma", "Oma", null)
        coVerify { admin.createUser("oma", "Oma", null) }
        assertEquals("u2", v.ui.value.created?.id)
    }

    @Test fun `remove device now steps up for that authenticator and is hidden for admins`() = runTest {
        grant(StepUpAction.ADMIN_AUTHENTICATOR_REMOVE, "kd1")
        coEvery { admin.user("u2") } returns detail
        val v = AdminUserViewModel(admin, stepUp, "u2", myUserId = { "u1" })
        v.load()
        assertTrue(v.ui.value.canRemoveDevices)
        v.removeNow(activity, detail.keyDevices.single())
        coVerify { stepUp.stepUp(activity, StepUpAction.ADMIN_AUTHENTICATOR_REMOVE, "kd1") }
        coVerify { admin.removeAuthenticator("u2", "kd1") }

        coEvery { admin.user("u3") } returns detail.copy(id = "u3", isAdmin = true)
        val a = AdminUserViewModel(admin, stepUp, "u3", myUserId = { "u1" })
        a.load()
        assertFalse(a.ui.value.canRemoveDevices)
        coEvery { admin.removeAuthenticator("u2", "kd1") } throws apiError(403, "ADMIN_TARGET_IS_ADMIN")
        v.removeNow(activity, detail.keyDevices.single())
        assertEquals(R.string.admin_target_is_admin, v.ui.value.message)
    }

    @Test fun `may approve switch steps up with the user as target`() = runTest {
        grant(StepUpAction.ADMIN_MAY_APPROVE, "u2")
        coEvery { admin.user("u2") } returns detail
        val v = AdminUserViewModel(admin, stepUp, "u2", myUserId = { "u1" })
        v.load()
        v.setMayApprove(activity, true)
        coVerify { stepUp.stepUp(activity, StepUpAction.ADMIN_MAY_APPROVE, "u2") }
        coVerify { admin.setMayApprove("u2", true) }
    }

    @Test fun `no setup a phone for oneself`() = runTest {
        coEvery { admin.user("u1") } returns detail.copy(id = "u1")
        val v = AdminUserViewModel(admin, stepUp, "u1", myUserId = { "u1" })
        v.load()
        assertFalse(v.ui.value.canSetUpPhone)
    }
}
