package io.wolftown.kaiku.ui.settings

import android.app.Activity
import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.coVerifyOrder
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.PasskeyApi
import io.wolftown.kaiku.data.api.PasskeyApiException
import io.wolftown.kaiku.data.auth.PasskeyCredentials
import io.wolftown.kaiku.data.auth.PasskeyOutcome
import io.wolftown.kaiku.data.auth.PasskeyStepUp
import io.wolftown.kaiku.data.auth.StepUpProvider
import io.wolftown.kaiku.domain.model.CeremonyStart
import io.wolftown.kaiku.domain.model.Passkey
import io.wolftown.kaiku.domain.model.PasskeyList
import io.wolftown.kaiku.domain.model.PasskeyRemoved
import io.wolftown.kaiku.domain.model.StepUpGrant
import java.util.Optional
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class PasskeysViewModelTest {
    private val dispatcher = StandardTestDispatcher()
    private val api: PasskeyApi = mockk(relaxed = true)
    private val credentials: PasskeyCredentials = mockk(relaxed = true)
    private val passkeyStepUp: PasskeyStepUp = mockk()
    private val device: StepUpProvider = mockk()
    private val activity: Activity = mockk(relaxed = true)
    private val grant = StepUpGrant("passkey.add", "", "passkey", "2026-11-05T10:00:00Z")
    private val active = Passkey("p-1", "Pixel 8", "active", "2026-11-01T10:00:00Z", backupState = true)
    private val pending = Passkey("p-2", "Tablet", "pending", "2026-11-01T10:00:00Z", readyAt = "2026-11-05T10:00:00Z")
    private val start = CeremonyStart("c-1", Json.parseToJsonElement("""{"publicKey":{"challenge":"AQ"}}""").jsonObject)

    @Before
    fun setUp() {
        Dispatchers.setMain(dispatcher)
        coEvery { passkeyStepUp.stepUp(any(), any(), any()) } returns Result.success(grant)
        coEvery { device.stepUp(any(), any(), any()) } returns Result.success(grant.copy(via = "device"))
        coEvery { api.registerStart(any()) } returns start
        coEvery { credentials.create(any(), any()) } returns PasskeyOutcome.Success("""{"id":"bmV3"}""")
    }

    @After
    fun tearDown() = Dispatchers.resetMain()

    private fun vm(list: List<Passkey>, withDevice: Boolean = false): PasskeysViewModel {
        coEvery { api.list() } returns PasskeyList("chat.example.com", "dQ", list)
        return PasskeysViewModel(api, credentials, passkeyStepUp, if (withDevice) Optional.of(device) else Optional.empty())
    }

    @Test
    fun `add with passkey steps up then registers`() = runTest(dispatcher) {
        val vm = vm(listOf(active))
        advanceUntilIdle()
        assertTrue(vm.state.value.canAdd)
        vm.add(activity, "Pixel 8")
        advanceUntilIdle()
        coVerifyOrder {
            passkeyStepUp.stepUp(activity, "passkey.add", "")
            api.registerStart("Pixel 8")
            credentials.create(activity, """{"challenge":"AQ"}""")
            api.registerFinish("c-1", any())
        }
    }

    @Test
    fun `add on a key device uses the device step-up provider`() = runTest(dispatcher) {
        val vm = vm(emptyList(), withDevice = true)
        advanceUntilIdle()
        assertTrue(vm.state.value.canAdd)
        vm.add(activity, "Pixel 8")
        advanceUntilIdle()
        coVerify { device.stepUp(activity, "passkey.add", "") }
        coVerify(exactly = 0) { passkeyStepUp.stepUp(any(), any(), any()) }
        coVerify { api.registerFinish("c-1", any()) }
    }

    @Test
    fun `no add without a passkey`() = runTest(dispatcher) {
        val vm = vm(listOf(pending))
        advanceUntilIdle()
        assertFalse(vm.state.value.canAdd)
    }

    @Test
    fun `remove steps up for that passkey then deletes and signals`() = runTest(dispatcher) {
        val removed = PasskeyRemoved("chat.example.com", "dQ", "Y3JlZA", listOf("b3RoZXI"))
        coEvery { api.delete("p-1") } returns removed
        val vm = vm(listOf(active))
        advanceUntilIdle()
        vm.remove(activity, active)
        advanceUntilIdle()
        coVerifyOrder {
            passkeyStepUp.stepUp(activity, "passkey.remove", "p-1")
            api.delete("p-1")
            credentials.signalRemoved("chat.example.com", "dQ", "Y3JlZA", listOf("b3RoZXI"))
        }
    }

    @Test
    fun `last passkey shows the add-another message`() = runTest(dispatcher) {
        coEvery { api.delete("p-1") } throws PasskeyApiException(HttpStatusCode.Conflict, "LAST_STRONG_AUTHENTICATOR")
        val vm = vm(listOf(active))
        advanceUntilIdle()
        vm.remove(activity, active)
        advanceUntilIdle()
        assertEquals(R.string.passkeys_error_last_strong, vm.state.value.errorRes)
    }

    @Test
    fun `pending passkey shows ready time`() = runTest(dispatcher) {
        val vm = vm(listOf(active, pending))
        advanceUntilIdle()
        val p = vm.state.value.passkeys.single { it.id == "p-2" }
        assertTrue(p.isPending)
        assertEquals("2026-11-05T10:00:00Z", p.readyAt)
    }

    @Test
    fun `a removed passkey reloads the list and a pending one keeps its ready time`() = runTest(dispatcher) {
        // Review Argon-a m2.
        coEvery { passkeyStepUp.stepUp(any(), "passkey.remove", any()) } returns
            Result.failure(io.wolftown.kaiku.data.auth.PasskeyFailure.Removed)
        val vm = vm(listOf(active))
        advanceUntilIdle()
        vm.remove(activity, active)
        advanceUntilIdle()
        coVerify(exactly = 2) { api.list() }
        coEvery { passkeyStepUp.stepUp(any(), "passkey.remove", any()) } returns
            Result.failure(io.wolftown.kaiku.data.auth.PasskeyFailure.Pending("2026-11-05T10:00:00Z"))
        vm.remove(activity, active)
        advanceUntilIdle()
        assertEquals(R.string.passkeys_error_ready_at, vm.state.value.errorRes)
        assertEquals("2026-11-05T10:00:00Z", vm.state.value.readyAt)
    }
}
