package io.wolftown.kaiku.ui.approval

import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.viewModelScope
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.domain.model.PcSetupPhoneStatus
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.KeyDeviceFixture
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.SigFixtures.hex
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File
import java.io.IOException

@OptIn(ExperimentalCoroutinesApi::class)
class PcSetupApprovalViewModelTest {

    private val activity: FragmentActivity = mockk(relaxed = true)
    private val tokenBytes = SigFixtures.bytes(0x20..0x3f)
    private val payload = ScanPayload.PcSetup(SigFixtures.ORIGIN, SigFixtures.b64u(tokenBytes))
    private val api = FakeApprovalApi()
    private val vectors = Json.parseToJsonElement(
        javaClass.classLoader!!.getResource("contract/pc_setup_sig_vectors.json")!!.readText(),
    ).jsonObject["vectors"]!!.jsonArray.map { it.jsonObject }
    private val p01 = vectors[0]
    private val p02 = vectors[1]

    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun ctxJson(v: kotlinx.serialization.json.JsonObject = p01, edit: (String) -> String = { it }) =
        edit(v["context"]!!.toString())

    private fun TestScope.vm(
        uv: UvResult? = null,
        active: Boolean = true,
        context: String = ctxJson(),
        n: Int = 16,
        now: Long = SigFixtures.EXPIRES_AT - 30,
    ): Pair<PcSetupApprovalViewModel, KeyDeviceFixture> {
        val f = KeyDeviceFixture(backgroundScope, uv, active, now)
        api.nextChallenge = SigFixtures.challengeFor(SigKind.PC_SETUP, n, context, wireKind = "pc_setup")
        api.pcClock = { testScheduler.currentTime }
        return PcSetupApprovalViewModel(api, f.signer, f.repo, { SigFixtures.DEVICE_ID }, appScope = backgroundScope) to f
    }

    @Test fun `without a key device nothing is sent`() = runTest {
        val (v, _) = vm(active = false)
        v.start(payload)
        assertEquals(PcSetupUi.Error(R.string.scan_not_a_key, rescan = false), v.ui.value)
        assertTrue(api.calls.isEmpty())
    }

    @Test fun `context shows only the verified fields`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        val ui = v.ui.value as PcSetupUi.Context
        assertEquals("Firefox on Linux", ui.ctx.clientDescription)
        assertEquals("Hamburg, DE", ui.ctx.place)
        assertEquals(listOf("claimPc"), api.calls)
    }

    @Test fun `approve signs the P01 bytes and shows the code only after the answer`() = runTest {
        val (v, f) = vm()
        v.start(payload)
        assertTrue(v.ui.value is PcSetupUi.Context)
        v.approve(activity)
        assertEquals(PcSetupUi.Code("123456"), v.ui.value)
        assertEquals(p01["message_hex"]!!.jsonPrimitive.content, f.verifier.signedMessages.single().hex())
        val (decision, response, _) = api.pcLastAnswer!!
        assertEquals("approve", decision)
        assertEquals(payload.tokenB64u, response)
        assertEquals(1, f.verifier.prompts.size)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `the code does not appear when the screen lock is cancelled`() = runTest {
        val (v, _) = vm(uv = UvResult.Cancelled)
        v.start(payload)
        v.approve(activity)
        assertTrue(v.ui.value is PcSetupUi.Context)
        assertTrue(api.calls.none { it.startsWith("answerPc") })
    }

    @Test fun `i didn't start this asks first then signs the P02 bytes`() = runTest {
        val (v, f) = vm(context = ctxJson(p02), n = 17)
        v.start(payload)
        v.askReport()
        assertTrue(v.ui.value is PcSetupUi.ConfirmReport)
        assertTrue(f.verifier.prompts.isEmpty())
        v.cancelReport()
        assertTrue(v.ui.value is PcSetupUi.Context)
        v.askReport()
        v.confirmReport(activity)
        assertEquals(PcSetupUi.Finished(R.string.pc_setup_reported, ok = false), v.ui.value)
        assertEquals(p02["message_hex"]!!.jsonPrimitive.content, f.verifier.signedMessages.single().hex())
        assertEquals("report", api.pcLastAnswer!!.first)
    }

    @Test fun `no refuses without a prompt`() = runTest {
        val (v, f) = vm()
        v.start(payload)
        v.deny()
        assertEquals(listOf("claimPc", "refusePc:${SigFixtures.challengeId(16)}"), api.calls)
        assertTrue(f.verifier.prompts.isEmpty())
        assertEquals(PcSetupUi.Finished(R.string.pc_setup_denied, ok = false), v.ui.value)
    }

    @Test fun `a changed context is refused released and never prompted`() = runTest {
        val (v, f) = vm()
        api.nextChallenge = api.nextChallenge!!.copy(
            context = SigFixtures.json(ctxJson { it.replace("Firefox on Linux", "Your own phone") }),
        )
        v.start(payload)
        assertEquals(PcSetupUi.Error(R.string.approval_mismatch, rescan = true), v.ui.value)
        assertEquals("release:${SigFixtures.challengeId(16)}", api.calls.last())
        assertTrue(f.verifier.prompts.isEmpty())
    }

    @Test fun `a foreign account gets its own text`() = runTest {
        val (v, _) = vm()
        api.failWith = apiError(409, "PC_SETUP_OTHER_ACCOUNT")
        v.start(payload)
        assertEquals(PcSetupUi.Error(R.string.pc_setup_other_account, rescan = false), v.ui.value)
    }

    @Test fun `a rotated code says to scan again`() = runTest {
        val (v, _) = vm()
        api.failWith = apiError(404, "LOGIN_CODE_STALE")
        v.start(payload)
        assertEquals(PcSetupUi.Error(R.string.pc_setup_code_changed, rescan = true), v.ui.value)
    }

    @Test fun `back releases an undecided claim but never after a decision`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.back()
        testScheduler.runCurrent()
        assertEquals("release:${SigFixtures.challengeId(16)}", api.calls.last())
        val (w, _) = vm()
        api.calls.clear()
        w.start(payload)
        w.approve(activity)
        w.back()
        testScheduler.runCurrent()
        assertTrue(api.calls.none { it.startsWith("release") })
        w.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `back on the code screen asks before the code is gone`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        assertTrue(v.requestBack()) // before a decision: leave at once
        v.approve(activity)
        assertFalse(v.requestBack())
        assertTrue(v.leaveAsked.value)
        assertEquals(PcSetupUi.Code("123456"), v.ui.value)
        v.stay()
        assertFalse(v.leaveAsked.value)
        assertEquals(PcSetupUi.Code("123456"), v.ui.value)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `the leave question goes away when the code is no longer needed`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        assertFalse(v.requestBack())
        assertTrue(v.leaveAsked.value)
        api.pcStatuses.add(PcSetupPhoneStatus("enrolled"))
        advanceTimeBy(1_100)
        assertEquals(PcSetupUi.Entered, v.ui.value)
        assertFalse(v.leaveAsked.value)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `code survives lifecycle stop`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        // The screen stops (phone locked, app in the background): nothing touches the ViewModel.
        advanceTimeBy(30_000)
        assertEquals(PcSetupUi.Code("123456"), v.ui.value)
        // Even the screen's Back handler for a decided attempt keeps the state until it is cleared.
        v.back()
        assertEquals(PcSetupUi.Code("123456"), v.ui.value)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `the phone's poll is a long-poll`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        advanceTimeBy(1_100)
        assertTrue(api.pcStatusWaits.isNotEmpty())
        assertTrue(api.pcStatusWaits.all { it == io.wolftown.kaiku.data.api.LongPoll.WAIT_SECONDS })
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `polls at most once a second`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        val start = testScheduler.currentTime
        advanceTimeBy(5_500)
        assertEquals(5, api.pcStatusCalls)
        api.pcStatusTimes.zipWithNext { a, b -> assertTrue(b - a >= 1_000) }
        assertTrue(api.pcStatusTimes.first() - start >= 1_000)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `network loss keeps the code and resumes polling`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        api.pcStatuses.addAll(listOf(IOException("offline"), IOException("offline"), IOException("offline")))
        advanceTimeBy(3_500)
        assertEquals(PcSetupUi.Code("123456"), v.ui.value)
        assertEquals(3, api.pcStatusCalls)
        // Back online: the poll goes on and an end is still noticed.
        api.pcStatuses.add(PcSetupPhoneStatus("failed", "too_many_codes"))
        advanceTimeBy(1_000)
        assertEquals(4, api.pcStatusCalls)
        assertEquals(PcSetupUi.Finished(R.string.pc_setup_too_many_codes, ok = false), v.ui.value)
        advanceTimeBy(5_000)
        assertEquals(4, api.pcStatusCalls)
    }

    @Test fun `the poll ends are told and the code is dropped`() = runTest {
        val cases = listOf(
            PcSetupPhoneStatus("completed") to PcSetupUi.Finished(R.string.pc_setup_done, ok = true),
            PcSetupPhoneStatus("expired", "session_ended") to PcSetupUi.Finished(R.string.pc_setup_session_ended, ok = false),
            PcSetupPhoneStatus("expired", "code_expired") to PcSetupUi.Finished(R.string.pc_setup_code_expired, ok = false),
            PcSetupPhoneStatus("cancelled", "cancelled") to PcSetupUi.Finished(R.string.pc_setup_cancelled, ok = false),
            PcSetupPhoneStatus("expired", "device_revoked") to PcSetupUi.Finished(R.string.pc_setup_failed, ok = false),
            PcSetupPhoneStatus("expired", "no_key_device") to PcSetupUi.Finished(R.string.pc_setup_failed, ok = false),
            PcSetupPhoneStatus("expired", "account_banned") to PcSetupUi.Finished(R.string.pc_setup_failed, ok = false),
            PcSetupPhoneStatus("expired", "may_approve_off") to PcSetupUi.Finished(R.string.scan_may_approve_off, ok = false),
            PcSetupPhoneStatus("enrolled") to PcSetupUi.Entered,
        )
        cases.forEach { (status, expected) ->
            val (v, _) = vm()
            api.pcStatuses.clear()
            v.start(payload)
            v.approve(activity)
            api.pcStatuses.add(status)
            advanceTimeBy(1_100)
            assertEquals(status.toString(), expected, v.ui.value)
            v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
        }
    }

    @Test fun `a lost answer says to start again on the computer`() = runTest {
        val (v, _) = vm()
        api.pcAnswerFailure = IOException("reset")
        v.start(payload)
        v.approve(activity)
        assertEquals(PcSetupUi.Error(R.string.pc_setup_answer_lost, rescan = false), v.ui.value)
    }

    @Test fun `an answer after the session ended reads the reason from the attempt`() = runTest {
        val (v, _) = vm()
        api.pcAnswerFailure = apiError(409, "APPROVAL_STATE")
        api.pcStatuses.add(PcSetupPhoneStatus("expired", "session_ended"))
        v.start(payload)
        v.approve(activity)
        assertEquals(PcSetupUi.Finished(R.string.pc_setup_session_ended, ok = false), v.ui.value)
    }

    @Test fun `the code and the token never appear in a toString`() {
        assertEquals("Code", PcSetupUi.Code("123456").toString())
        assertFalse(PcSetupUi.Code("123456").toString().contains("123456"))
        assertFalse(payload.toString().contains(payload.tokenB64u))
    }

    @Test fun `the screen keeps the spec wording`() {
        val xml = File("src/main/res/values/strings.xml").readText()
        listOf(
            "Give this computer full access to your account?",
            "Only continue if you started this on a computer in front of you, and nobody asked you to.",
            "It could change your sign-in methods and sign you out elsewhere.",
            "End that computer\\'s session?",
            "Type this code on the computer in front of you.",
            "Never tell it to anyone or type it on a site someone sent you.",
            "This computer is signed in with a different account.",
            "This code isn\\'t valid or has just changed. Scan the code on your computer again.",
        ).forEach { assertTrue(it, xml.contains(it)) }
    }

    @Test fun `a failed passkey on the pc says it didn't work and never done`() = runTest {
        listOf(PcSetupPhoneStatus("expired", "enrollment_expired"), PcSetupPhoneStatus("failed", "too_many_codes")).forEach { st ->
            val (v, _) = vm()
            api.pcStatuses.clear()
            v.start(payload)
            v.approve(activity)
            api.pcStatuses.add(PcSetupPhoneStatus("enrolled"))
            advanceTimeBy(1_100)
            api.pcStatuses.add(st)
            advanceTimeBy(1_000)
            val ui = v.ui.value as PcSetupUi.Finished
            assertFalse(ui.ok)
            assertTrue(ui.msg != R.string.pc_setup_done)
            if (st.reason == "enrollment_expired") assertEquals(R.string.pc_setup_failed, ui.msg)
        }
    }

    @Test fun `enrolled keeps polling until a later end`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        api.pcStatuses.add(PcSetupPhoneStatus("enrolled"))
        advanceTimeBy(1_100)
        assertEquals(PcSetupUi.Entered, v.ui.value)
        val calls = api.pcStatusCalls
        advanceTimeBy(3_000)
        assertEquals(calls + 3, api.pcStatusCalls)
        assertEquals(PcSetupUi.Entered, v.ui.value)
        api.pcStatuses.add(PcSetupPhoneStatus("completed"))
        advanceTimeBy(1_000)
        assertEquals(PcSetupUi.Finished(R.string.pc_setup_done, ok = true), v.ui.value)
    }

    @Test fun `a refused answer releases the claim before offering scan again`() = runTest {
        val (v, _) = vm()
        api.pcAnswerFailure = apiError(400, "SIGNATURE_INVALID")
        v.start(payload)
        v.approve(activity)
        assertEquals(PcSetupUi.Error(R.string.approval_mismatch, rescan = true), v.ui.value)
        assertEquals("release:${SigFixtures.challengeId(16)}", api.calls.last())
    }

    @Test fun `a lost answer is not released`() = runTest {
        val (v, _) = vm()
        api.pcAnswerFailure = IOException("reset")
        v.start(payload)
        v.approve(activity)
        assertTrue(api.calls.none { it.startsWith("release") })
    }

    // Back while sending, unknown ends, poll errors, an expired challenge.

    @Test fun `back while the decision is being sent is swallowed and the code is shown after`() = runTest {
        val (v, _) = vm()
        val gate = kotlinx.coroutines.CompletableDeferred<Unit>()
        api.answerGate = gate
        v.start(payload)
        v.approve(activity)
        // The signature exists and the answer is on its way.
        assertEquals(PcSetupUi.Sending, v.ui.value)
        assertTrue(screenHandlesBack(v.ui.value))
        assertFalse(v.requestBack())
        assertFalse(v.leaveAsked.value)
        gate.complete(Unit)
        assertEquals(PcSetupUi.Code("123456"), v.ui.value)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `an unknown state or reason ends with the generic text and stops polling`() = runTest {
        val cases = listOf(
            PcSetupPhoneStatus("time_travel") to R.string.pc_setup_failed,
            PcSetupPhoneStatus("expired", "some_new_reason") to R.string.pc_setup_failed,
            // An unknown reason wins over a known state, live or ended.
            PcSetupPhoneStatus("failed", "some_new_reason") to R.string.pc_setup_failed,
            PcSetupPhoneStatus("refused", "some_new_reason") to R.string.pc_setup_failed,
            PcSetupPhoneStatus("reported", "some_new_reason") to R.string.pc_setup_failed,
            PcSetupPhoneStatus("code", "some_new_reason") to R.string.pc_setup_failed,
            PcSetupPhoneStatus("enrolled", "some_new_reason") to R.string.pc_setup_failed,
            PcSetupPhoneStatus("expired", "timeout") to R.string.pc_setup_timeout,
            PcSetupPhoneStatus("cancelled", null) to R.string.pc_setup_cancelled,
            PcSetupPhoneStatus("cancelled", "restarted") to R.string.pc_setup_cancelled,
        )
        cases.forEach { (status, expected) ->
            val (v, _) = vm()
            api.pcStatuses.clear()
            v.start(payload)
            v.approve(activity)
            api.pcStatuses.add(status)
            advanceTimeBy(1_100)
            assertEquals(status.toString(), PcSetupUi.Finished(expected, ok = false), v.ui.value)
            val calls = api.pcStatusCalls
            advanceTimeBy(5_000)
            assertEquals(status.toString(), calls, api.pcStatusCalls)
            v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
        }
    }

    @Test fun `a 404 without APPROVAL_NOT_FOUND keeps the code and stops asking`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        api.pcStatuses.add(io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.NotFound, "Not Found"))
        advanceTimeBy(1_100)
        assertEquals(PcSetupUi.Code("123456"), v.ui.value)
        val calls = api.pcStatusCalls
        advanceTimeBy(5_000)
        assertEquals(calls, api.pcStatusCalls)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `completed is success whatever reason comes with it`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        api.pcStatuses.add(PcSetupPhoneStatus("completed", "brand_new"))
        advanceTimeBy(1_100)
        assertEquals(PcSetupUi.Finished(R.string.pc_setup_done, ok = true), v.ui.value)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `a vanished attempt closes the leave question`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        assertFalse(v.requestBack())
        api.pcStatuses.add(apiError(404, "APPROVAL_NOT_FOUND"))
        advanceTimeBy(1_100)
        assertEquals(PcSetupUi.Finished(R.string.pc_setup_cancelled, ok = false), v.ui.value)
        assertFalse(v.leaveAsked.value)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `a 429 waits for Retry-After and a refusal that cannot change ends`() = runTest {
        val (v, _) = vm()
        v.start(payload)
        v.approve(activity)
        api.pcStatuses.add(apiError(429, "TOO_MANY_ATTEMPTS", io.wolftown.kaiku.data.api.ApiErrorFields(retryAfterSeconds = 10)))
        api.pcStatuses.add(PcSetupPhoneStatus("code"))
        advanceTimeBy(1_100)
        assertEquals(1, api.pcStatusCalls)
        advanceTimeBy(8_000)
        assertEquals(1, api.pcStatusCalls)
        advanceTimeBy(2_500)
        assertEquals(2, api.pcStatusCalls)
        v.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()

        val (w, _) = vm()
        api.pcStatuses.clear()
        w.start(payload)
        w.approve(activity)
        api.pcStatuses.add(apiError(403, "KEY_DEVICE_REQUIRED"))
        advanceTimeBy(1_100)
        // An answer that says nothing about the attempt: polling stops, the code stays.
        assertEquals(PcSetupUi.Code("123456"), w.ui.value)
        val calls = api.pcStatusCalls
        advanceTimeBy(5_000)
        assertEquals(calls, api.pcStatusCalls)
        w.viewModelScope.coroutineContext[kotlinx.coroutines.Job]!!.cancel()
    }

    @Test fun `a challenge that expired before the tap says it took too long`() = runTest {
        val (v, _) = vm(now = SigFixtures.EXPIRES_AT + 600)
        v.start(payload)
        assertEquals(PcSetupUi.Error(R.string.approval_expired, rescan = true), v.ui.value)
    }

    @Test fun `the screen hands Back to the system during the screen lock and handles it otherwise`() {
        // During the prompt the system handles Back; while sending and on the code screen the screen does.
        assertFalse(screenHandlesBack(PcSetupUi.Working))
        assertTrue(screenHandlesBack(PcSetupUi.Sending))
        assertTrue(screenHandlesBack(PcSetupUi.Code("123456")))
        assertTrue(screenHandlesBack(PcSetupUi.Claiming))
    }

    @Test fun `a challenge that runs out while the person reads says it took too long at the tap`() = runTest {
        val (v, f) = vm()
        v.start(payload)
        assertTrue(v.ui.value is PcSetupUi.Context)
        f.now = SigFixtures.EXPIRES_AT + 600
        v.approve(activity)
        assertEquals(PcSetupUi.Error(R.string.approval_expired, rescan = true), v.ui.value)
    }
}
