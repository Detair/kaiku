package io.wolftown.kaiku.ui.auth.setup

import androidx.fragment.app.FragmentActivity
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.domain.model.ClaimStatus
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.testutil.FakeEnrollmentApi
import io.wolftown.kaiku.testutil.InMemoryKeyDeviceStore
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Before
import org.junit.Test
import java.time.Instant

@OptIn(ExperimentalCoroutinesApi::class)
class SetupPendingViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val enroll = FakeEnrollmentApi()
    private val deviceSignIn: DeviceSignIn = mockk()
    private val readyAt = Instant.parse("2026-11-04T12:00:00Z")

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun pendingDevice() = LocalKeyDevice(null, FakeEnrollmentApi.SUBJECT_ID, SigFixtures.ORIGIN, UvClass.PER_USE,
        "55555555-5555-4555-8555-555555555555", "claim-secret", "Oma")

    @Test fun `pending setup resumes at app start and signs in when active`() = runTest {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.authState.setLoggedOut()
        val store = InMemoryKeyDeviceStore()
        KeyDeviceRepository(store, f.keyStore, f.authState, backgroundScope).markPending(pendingDevice(), readyAt)
        // App restart: a new repository instance reads the pending setup.
        val repo = KeyDeviceRepository(store, f.keyStore, f.authState, backgroundScope)
        assertEquals(KeyDeviceState.Pending(readyAt, "55555555-5555-4555-8555-555555555555"), repo.current())
        coEvery { deviceSignIn.signIn(any()) } returns DeviceSignInResult.Success
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "pending", readyAt.toString())
        enroll.statuses += ClaimStatus(EnrollmentState.DONE, "active")

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn)
        v.check()
        assertEquals(SetupUi.Pending(readyAt), v.ui.value)
        v.check()
        assertEquals(SetupUi.SigningIn, v.ui.value)
        v.signIn(activity)
        coVerify { deviceSignIn.signIn(activity) }
        assertEquals(SetupUi.Done, v.ui.value)
    }

    @Test fun `cancelled pending setup shows the cancelled screen and forgets the key`() = runTest {
        val f = KeyDeviceFixture(backgroundScope, active = false)
        f.authState.setLoggedOut()
        val repo = KeyDeviceRepository(InMemoryKeyDeviceStore(), f.keyStore, f.authState, backgroundScope)
        repo.markPending(pendingDevice(), readyAt)
        enroll.statuses += ClaimStatus(EnrollmentState.CANCELLED)

        val v = SetupPendingViewModel(enroll.api, repo, deviceSignIn)
        v.check()
        assertEquals(SetupUi.Ended(R.string.setup_cancelled, canRetry = false), v.ui.value)
        assertEquals(KeyDeviceState.None, repo.current())
        assertFalse(f.keyStore.hasKey())
    }
}
