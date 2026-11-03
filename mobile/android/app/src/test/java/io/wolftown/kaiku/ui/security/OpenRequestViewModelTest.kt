package io.wolftown.kaiku.ui.security

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.domain.model.AccountRequestSummary
import io.wolftown.kaiku.domain.model.CancelVia
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.domain.model.StepUpGrant
import io.wolftown.kaiku.testutil.FakeSecurityApi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class OpenRequestViewModelTest {
    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    @Test fun `open request prompt shows at app open and cancel steps up`() = runTest {
        val activity: FragmentActivity = mockk(relaxed = true)
        val api = FakeSecurityApi()
        val request = AccountRequestSummary("rq1", AccountRef("u1", "alice"), kind = "self", purpose = "recovery",
            state = "waiting", cancel = CancelVia("step_up"))
        api.me = api.me.copy(openRequest = request, prompts = listOf("open_request"))
        api.cancelResult = request.copy(state = "cancelled")
        val stepUp: DeviceStepUp = mockk()
        coEvery { stepUp.stepUp(activity, StepUpAction.REQUEST_CANCEL, "rq1", any()) } returns
            StepUpOutcome.Granted(StepUpGrant("request.cancel", "rq1", "device", "t"))
        val v = OpenRequestViewModel(api, stepUp)

        v.load()
        assertEquals(request, v.pending.value)
        v.cancel(activity)
        coVerify { stepUp.stepUp(activity, StepUpAction.REQUEST_CANCEL, "rq1", any()) }
        assertTrue("cancelRequest:rq1" in api.calls)
        assertNull(v.pending.value)
    }

    @Test fun `a completed or missing request shows nothing`() = runTest {
        val api = FakeSecurityApi()
        val v = OpenRequestViewModel(api, mockk())
        v.load()
        assertNull(v.pending.value)
        api.me = api.me.copy(openRequest = AccountRequestSummary("rq1", AccountRef("u1", "alice"), state = "completed"))
        v.load()
        assertNull(v.pending.value)
    }
}
