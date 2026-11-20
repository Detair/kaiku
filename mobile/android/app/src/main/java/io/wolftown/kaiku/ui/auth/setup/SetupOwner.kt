package io.wolftown.kaiku.ui.auth.setup

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ChallengeRejected
import io.wolftown.kaiku.approval.Decision
import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.DeviceSuitabilityChecker
import io.wolftown.kaiku.approval.EnrollPopContext
import io.wolftown.kaiku.approval.GeneratedKey
import io.wolftown.kaiku.approval.KaikuSig
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.SignOutcome
import io.wolftown.kaiku.approval.UnsuitableReason
import io.wolftown.kaiku.approval.UvDecision
import io.wolftown.kaiku.approval.UvPrompt
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.data.api.LongPoll
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.SignInPurpose
import io.wolftown.kaiku.data.keydevice.Ask
import io.wolftown.kaiku.data.keydevice.Change
import io.wolftown.kaiku.data.keydevice.KeyAnswers
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.keydevice.NewKey
import io.wolftown.kaiku.data.keydevice.ServerAnswer
import io.wolftown.kaiku.data.keydevice.ServerKeyState
import io.wolftown.kaiku.data.keydevice.StoredKeyDevice
import io.wolftown.kaiku.data.keydevice.Verdict
import io.wolftown.kaiku.di.SetupCoroutineScope
import io.wolftown.kaiku.domain.CodeFormat
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.EnrollmentClaim
import io.wolftown.kaiku.domain.model.EnrollmentState
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.CoroutineStart
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.async
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.util.Base64
import java.util.UUID
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton
import kotlin.coroutines.cancellation.CancellationException

/**
 * The one setup of this phone, for the whole app (SM2 on the new phone, F5a/F5c, L2-A1, L2-A13):
 * origin check, open (shows the server name to an unconfigured app), suitability, K_approve with
 * the enrolment nonce as attestation challenge, PoP challenge, the subject question, the signed
 * claim, the code, the status long-poll, then device-key sign-in (full) or the 24-hour wait; and,
 * for a setup found stored at a start, the wait and its checks. With a registration
 * [SetupEntry.ticket] the claim is the app's own (no code).
 *
 * The rules, and what makes each hold (the tests that prove it are named in brackets):
 *
 * 1. One setup per app. This object is a singleton; the scanner screen, the link screen and the
 *    waiting screen are views of it ([attach]) and hold no setup of their own. There is one
 *    [attempt], one [running] step and one screen state, so no second setup exists that could
 *    start, continue, sign, save or forget. [SetupOwnerTest: `two screens show one setup`,
 *    `a link opened while the scanned setup shows its question starts nothing`]
 * 2. Links. Every setup link goes through [offer] (opened) and [start] (delivered to a screen).
 *    While a setup is held (started and not at an end: stopped, failed, finished) the same link
 *    again changes nothing and every other link is only noted ([linkIgnored]), whatever the server
 *    or the screen, the waiting screen included. Once the setup has ended on any screen, a link
 *    opened again starts. [SetupOwnerTest: `a link opened over the waiting screen is noted there
 *    and not used`, `after the waiting screen reported the end, a link opened again starts`;
 *    SetupFlowViewModelTest: the `a link …` and `… is not used` tests,
 *    `the running link delivered again does not start it over`]
 * 3. One step. At most one step runs ([running]); it is installed before its first line can run
 *    and stopped before the flow changes. A step that is no longer the running one sends no
 *    request, writes nothing and changes no screen: it checks [live] after every suspension point.
 *    [SetupFlowViewModelTest: `a step that starts the next step at once is still the one Cancel stops`]
 * 4. Storage only by compare-and-set. The record and the key are reached only through
 *    [KeyDeviceRepository]: a key is made only while no record exists and is adopted by the live
 *    step or deleted in the same uninterruptible block; the first record is written only where
 *    none is stored and the alias holds this attempt's key; every later write and every forgetting
 *    names the record this setup stored. [KeyDeviceRepositoryTest, KeyVaultInterleavingTest;
 *    SetupFlowViewModelTest: `a key made while the setup was stopped is deleted`; SetupOwnerTest:
 *    `nothing is sent when the claim cannot be recorded first`,
 *    `a key that is no longer this setup's key signs no claim and writes no record`]
 * 5. Forgetting only on listed answers. A stored record, and with it the key, is forgotten only by
 *    [KeyAnswers.settle], on an answer of [KeyAnswers.rows], asked on the record's own server. A
 *    claim is sent once per challenge and never again; an answer that does not settle it leaves the
 *    record `CLAIMING`, and [reconcileClaim] asks. [KeyAnswersTest, KeyForgetCallSitesTest;
 *    SetupOwnerTest: `a claim refused for good forgets its own key and record`,
 *    `any other answer to a claim keeps the key and the record`,
 *    `an undecided claim answer is checked with the enrolment, never sent again`,
 *    `a claim that times out and was stored is recovered, not replayed into a refusal`]
 * 6. The server. The app adopts a link's server only through [SetupEnvironment.adoptServer], which
 *    refuses once the app is tied to a server or holds a record; it is put back when the adopting
 *    setup is abandoned. [SetupOwnerTest: the two `a server question answered late adopts nothing …`
 *    tests; OriginPinTest: `a setup adopts its server only while the app is tied to none and holds
 *    no key record`]
 * 7. Bounded. The claim and the final sign-in are never stopped by a link, Back or Cancel; the
 *    claim's answer counts as not known after [CLAIM_TIMEOUT_MS], and the screen leaves the final
 *    sign-in after [SIGN_IN_TIMEOUT_MS] whatever the sign-in still has to finish.
 *    [SetupFlowViewModelTest: `a claim whose answer never comes ends as not sure after a while`;
 *    SetupOwnerTest: `the screen leaves the final sign-in at its limit, whatever the sign-in still
 *    has to finish`]
 */
@Singleton
class SetupOwner internal constructor(
    private val enrollmentApi: EnrollmentApi,
    private val suitability: DeviceSuitabilityChecker,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val deviceIds: DeviceIdProvider,
    private val deviceSignIn: DeviceSignIn,
    private val env: SetupEnvironment,
    private val probe: KeyDeviceProbe,
    private val answers: KeyAnswers,
    /** Lives as long as the app; steps run on the main thread. */
    private val scope: CoroutineScope,
    /** The suitability probe and key generation (StrongBox, attestation) take seconds: never on Main. */
    private val keyDispatcher: CoroutineDispatcher,
) {

    @Inject
    constructor(
        enrollmentApi: EnrollmentApi,
        suitability: DeviceSuitabilityChecker,
        signer: DeviceSigner,
        keyDevices: KeyDeviceRepository,
        deviceIds: DeviceIdProvider,
        deviceSignIn: DeviceSignIn,
        env: SetupEnvironment,
        probe: KeyDeviceProbe,
        answers: KeyAnswers,
        @SetupCoroutineScope scope: CoroutineScope,
    ) : this(enrollmentApi, suitability, signer, keyDevices, deviceIds, deviceSignIn, env, probe, answers, scope, Dispatchers.Default)

    companion object {
        /** The longest the signed claim may take before its answer counts as not known. */
        const val CLAIM_TIMEOUT_MS = 30_000L
        /** The longest the screen waits for the final sign-in (requests and the prompt, whose challenge lasts 120 s). */
        const val SIGN_IN_TIMEOUT_MS = 180_000L
        private val logger: Logger = Logger.getLogger("SetupFlow")
        private const val RETRY_MS = 2_000L
        private const val RETRY_MAX_MS = 30_000L
        private const val ASK_AGAIN_SLACK_MS = 5_000L
    }

    private val _ui = MutableStateFlow<SetupUi>(SetupUi.Opening)
    val ui: StateFlow<SetupUi> = _ui.asStateFlow()

    /**
     * One accepted setup code and its canonical origin, fixed for good. The rest is filled in by
     * this attempt's own steps only (a superseded step stops before writing, see [live]).
     */
    private class Attempt(val entry: SetupEntry, val origin: String) {
        /** Goes into every record this attempt stores. */
        val id: String = UUID.randomUUID().toString()
        var serverName: String = ""
        var nonce: ByteArray = ByteArray(0)
        var key: GeneratedKey? = null
        var pop: ApprovalChallenge? = null
        var subject: AccountRef? = null
        var popEnrollmentId: String? = null
        /** The record this attempt stored, as it last wrote it. */
        var rec: StoredKeyDevice? = null

        override fun toString() = "Attempt"
    }

    /** The running step: its job and the attempt it works for (rule 3). */
    private class Step(val attempt: Attempt?) {
        lateinit var job: Job
        override fun toString() = "Step"
    }

    private sealed interface KeyResult {
        class Made(val key: GeneratedKey) : KeyResult
        /** A claim is stored that is not resolved yet: the waiting screen takes over. */
        data object ClaimWaiting : KeyResult
        /** This phone became a key meanwhile. */
        data object AlreadyKey : KeyResult
        data object Unreadable : KeyResult
    }

    private var attempt: Attempt? = null
    private var running: Step? = null
    /** A setup code was handed in, or a stored setup opened, and nothing reset it since. */
    private var handed = false
    private var signingIn = false
    private var signInJob: Job? = null
    private var checkJob: Job? = null
    /** Screens showing this setup. */
    private var views = 0
    /** Key generation and deletion, one at a time (a stopped step may still be generating). */
    private val keyLock = Mutex()
    /** The server an unconfigured app had saved before this setup adopted the code's one. */
    private var replacedServer: String? = null
    private var adoptedServer = false

    val started: Boolean get() = handed

    private val _waiting = MutableStateFlow(false)
    /** The setup works on the stored record (a wait, or a claim being checked): the waiting screen shows it. */
    val waiting: StateFlow<Boolean> = _waiting.asStateFlow()

    private val _linkIgnored = MutableStateFlow(false)
    /** A setup link arrived while this setup was running and was not used (the screen says so). */
    val linkIgnored: StateFlow<Boolean> = _linkIgnored.asStateFlow()

    private val _scanReady = MutableStateFlow<UvDecision?>(null)
    /** C26 "Suitability": the screen lock and a hardware key store, checked before any scan. */
    val scanReady: StateFlow<UvDecision?> = _scanReady.asStateFlow()

    /** Runs the suitability check before the scanner opens (again after the screen-lock settings). */
    fun checkBeforeScan() {
        scope.launch { _scanReady.value = withContext(keyDispatcher) { suitability.check() } }
    }

    /** For hosts that switch between the scanner and this flow (SetupScanScreen). */
    private val _startedFlow = MutableStateFlow(false)
    val startedFlow: StateFlow<Boolean> = _startedFlow.asStateFlow()

    /** The signed claim is on its way: nothing may interrupt it (its answer carries the code). */
    private val claimInFlight: Boolean get() = _ui.value == SetupUi.Claiming

    private fun SetupUi.isEnd(): Boolean = this is SetupUi.Ended || this == SetupUi.Interrupted || this == SetupUi.Done

    /** A setup is held: started (from a link, or found stored) and not stopped, failed or finished yet (rule 2). */
    fun held(): Boolean = signingIn || ((attempt != null || _waiting.value) && !_ui.value.isEnd())

    /**
     * A screen shows this setup from now until it closes the handle. When a screen closes at an
     * end, the setup is cleared for the next one; when the last screen closes, whatever runs stops
     * and a key that was not claimed goes.
     */
    fun attach(): AutoCloseable {
        views++
        var open = true
        return AutoCloseable {
            if (open) {
                open = false
                views--
                if (views <= 0) {
                    views = 0
                    stop()
                    signInJob?.cancel()
                    abandonUnclaimed()
                    idle()
                } else if (!held()) {
                    // A screen closed at an end while another one stays: that one starts clean.
                    abandonUnclaimed()
                    idle()
                }
            }
        }
    }

    private fun idle() {
        stop()
        attempt = null
        handed = false
        _waiting.value = false
        _linkIgnored.value = false
        _startedFlow.value = false
        _ui.value = SetupUi.Opening
        // A server the ended setup adopted is the app's server now, or was put back by abandoning it.
        adoptedServer = false
        replacedServer = null
    }

    /**
     * A setup link the app was opened with. False: a setup is held, the link is not used (the
     * screen showing that setup says so) and nothing is to be navigated. True: no setup is held;
     * the link goes to the setup screen, which hands it to [start].
     */
    fun offer(e: SetupEntry): Boolean {
        if (!held()) return true
        if (e != attempt?.entry) _linkIgnored.value = true
        return false
    }

    /** Back to the scanner after an ended or abandoned setup; an unclaimed new key goes. */
    fun reset() {
        if (claimInFlight || signingIn) return
        stop()
        // A claim whose answer is unknown keeps its key and record: the next app start reconciles it.
        abandonUnclaimed()
        idle()
    }

    /** A stored claim (CLAIMING: answer unknown, CLAIMED: waiting for its code) that the server has not decided yet. */
    private fun unresolvedClaim(): StoredKeyDevice? =
        keyDevices.record.value?.takeIf { it.phase == KeyDevicePhase.CLAIMING || it.phase == KeyDevicePhase.CLAIMED }

    /** Ends the step in flight; always the first thing before the flow changes. */
    private fun stop() {
        running?.job?.cancel()
        running = null
    }

    /** Runs [block] as the one step in flight for [a]; installed before it runs, after the old one stopped. */
    private fun step(a: Attempt?, block: suspend () -> Unit): Job {
        stop()
        val s = Step(a)
        s.job = scope.launch(start = CoroutineStart.LAZY) { block() }
        running = s
        s.job.start()
        return s.job
    }

    /** Whether the coroutine with [job] is still the running step of [a], and [a] the current attempt. */
    private fun isLive(a: Attempt?, job: Job?): Boolean {
        val s = running
        return job != null && job.isActive && s != null && s.job === job && s.attempt === a && attempt === a
    }

    /**
     * After every suspension point of a step: it goes on only while it is still the running step and
     * [a] the current attempt; otherwise it ends here, without a request or a state change.
     */
    private suspend fun live(a: Attempt?) {
        val ctx = currentCoroutineContext()
        ctx.ensureActive()
        if (!isLive(a, ctx[Job])) throw CancellationException("superseded")
    }

    /** Sets the screen from a step, only while that step is live. */
    private suspend fun show(a: Attempt?, ui: SetupUi) {
        live(a)
        _ui.value = ui
    }

    /**
     * The one way in for a setup code, scanned or delivered as a link (rule 2). While a setup is
     * held it is never replaced: the same link again changes nothing, any other one is only noted.
     * Otherwise an unreadable link or one for another server than the pinned one ends with its
     * reason, and a usable one starts a new attempt.
     */
    fun start(e: SetupEntry) {
        val wasHeld = held()
        handed = true
        _startedFlow.value = true
        if (wasHeld) {
            if (e != attempt?.entry) _linkIgnored.value = true
            return
        }
        if (keyDevices.unreadable()) {
            stop()
            _ui.value = SetupUi.Ended(R.string.security_key_storage_unreadable, canRetry = false)
            return
        }
        val o = KaikuPayloads.canonicalOrigin(e.origin)
        val pinned = env.pinnedOrigin()
        if (o == null || (pinned != null && pinned != o)) {
            stop()
            _ui.value = SetupUi.Ended(if (o == null) R.string.setup_invalid else R.string.setup_other_server, canRetry = false)
            return
        }
        begin(e, o)
    }

    /** A new attempt for an accepted link (from [start], or the same link looked at again). */
    private fun begin(e: SetupEntry, o: String) {
        stop()
        val claim = unresolvedClaim()
        if (claim != null) {
            // Not a new setup: a claim of this phone is stored, and the server decides it first.
            // The attempt that made it stays (it still holds what the claim check needs).
            if (attempt?.entry != e) _linkIgnored.value = true
            return resumeClaim(claim)
        }
        abandonUnclaimed()
        val a = Attempt(e, o)
        attempt = a
        _waiting.value = false
        _linkIgnored.value = false
        when (keyDevices.current()) {
            is KeyDeviceState.Active -> return alreadyKey(a)
            is KeyDeviceState.Lapsed -> return startOnLapsedKey(a)
            is KeyDeviceState.Pending -> return end(R.string.setup_already_pending)
            KeyDeviceState.None -> Unit
        }
        _ui.value = SetupUi.Opening
        step(a) { open(a) }
    }

    /** A stored claim is looked at again: by the attempt that made it, or on the waiting screen. */
    private fun resumeClaim(claim: StoredKeyDevice) {
        val a = attempt
        if (a != null && claim.phase == KeyDevicePhase.CLAIMING && a.rec?.sameAs(claim) == true) {
            _ui.value = SetupUi.Opening
            step(a) { reconcileClaim(a, claim, null) }
        } else {
            _waiting.value = true
            _ui.value = SetupUi.Resume
        }
    }

    /**
     * A signed-in key phone: "already your key". Its session may be dead (an admin removed this
     * phone) without the app knowing yet; once the phone is no longer signed in with the key, the
     * same link is looked at again (a signed-out key then asks its server, [startOnLapsedKey]).
     */
    private fun alreadyKey(a: Attempt) {
        _ui.value = SetupUi.Ended(R.string.setup_already_key, canRetry = false)
        step(a) {
            // The state flow may still hold an older value: the phone's state now decides.
            keyDevices.state.first { keyDevices.current() !is KeyDeviceState.Active }
            live(a)
            begin(a.entry, a.origin)
        }
    }

    /**
     * A signed-out key phone may have lost its key on the server (an admin removed it). The key's
     * own server decides (the public device-login probe): only a key it does not know, or knows as
     * removed, is forgotten and the setup goes on; anything else keeps the key ("already your key").
     * The key is forgotten only if it is still the one that was asked about, still signed out.
     */
    private fun startOnLapsedKey(a: Attempt) {
        val rec = keyDevices.record.value ?: return end(R.string.setup_already_key)
        _ui.value = SetupUi.Opening
        step(a) {
            val st = probe.probe(rec.device.userId, rec.device.origin)
            live(a)
            if (st !is ServerKeyState.Gone) return@step show(a, SetupUi.Ended(R.string.setup_already_key, canRetry = false))
            if (keyDevices.record.value != null) {
                if (keyDevices.current() !is KeyDeviceState.Lapsed ||
                    !answers.settle(rec, originOf(rec), Ask.DEVICE_LOGIN_CHALLENGE, st.answer).forgotten
                ) {
                    return@step show(a, SetupUi.Ended(R.string.setup_already_key, canRetry = false))
                }
            }
            open(a)
        }
    }

    /** The pin is read now: forgetting a removed key above may have cleared it. */
    private suspend fun open(a: Attempt) {
        val pinned = env.pinnedOrigin()
        if (pinned != null && pinned != a.origin) return show(a, SetupUi.Ended(R.string.setup_other_server, canRetry = false))
        try {
            val open = enrollmentApi.open(a.entry.tokenB64u, a.origin)
            live(a)
            a.serverName = open.serverName
            a.nonce = Base64.getUrlDecoder().decode(open.enrollmentNonce)
            if (pinned == null && a.entry.source != SetupEntry.Source.TICKET) {
                show(a, SetupUi.ConfirmServer(open.serverName, hostOf(a.origin)))
            } else {
                checkSuitability(a)
            }
        } catch (c: CancellationException) {
            throw c
        } catch (x: Exception) {
            show(a, setupErrorFor(x))
        }
    }

    /**
     * The setup screen was restored without its setup code (L2-A1; the code never goes into
     * saved state): a stored setup continues on the waiting screen, which reconciles it with the
     * server; a finished one goes on; otherwise the person scans (or opens) the code again.
     */
    fun resumeWithoutEntry() {
        if (started) return
        handed = true
        _startedFlow.value = true
        when {
            keyDevices.current() is KeyDeviceState.Active -> _ui.value = SetupUi.Done
            keyDevices.record.value != null -> {
                _waiting.value = true
                _ui.value = SetupUi.Resume
            }
            else -> _ui.value = SetupUi.Interrupted
        }
    }

    /**
     * The waiting screen opens (L2-A13: a setup that waits 24 h, a claim whose code was not
     * confirmed before the app closed, or a claim whose answer never arrived). The state lives in
     * [KeyDeviceRepository]; [check] asks the server. A setup this object already shows stays.
     */
    fun openWaiting() {
        val now = _ui.value
        if (now != SetupUi.Resume && (attempt != null || _waiting.value) && !now.isEnd()) {
            if (now is SetupUi.Pending) _waiting.value = true
            return
        }
        handed = true
        _startedFlow.value = true
        _waiting.value = true
        _ui.value = storedUi()
    }

    private fun storedUi(): SetupUi {
        val s = keyDevices.record.value ?: return SetupUi.Ended(R.string.setup_cancelled, canRetry = false)
        return when (s.phase) {
            KeyDevicePhase.PENDING -> (keyDevices.current() as? KeyDeviceState.Pending)
                ?.let { SetupUi.Pending(it.readyAt) } ?: SetupUi.WaitingForCode
            KeyDevicePhase.ACTIVE -> SetupUi.SigningIn
            KeyDevicePhase.CLAIMED -> if (s.device.claimSecret != null) SetupUi.WaitingForCode else SetupUi.Opening
            KeyDevicePhase.CLAIMING -> SetupUi.Opening
        }
    }

    /**
     * Asks the server about the stored setup: at every app open and every 15 minutes while the
     * waiting screen is in front. With a claim secret the enrolment status says it; without one
     * (a self-claim, a claim whose answer never came) the key-device probe does. Active → the
     * device-key sign-in; ended → the reason, and the key is forgotten through [KeyAnswers.settle].
     */
    fun check() {
        if (!_waiting.value) {
            // The waiting screen is in front again after a setup shown above it ended: back to the stored one.
            if (attempt != null || held()) return
            openWaiting()
        }
        val rec = keyDevices.record.value ?: return
        val now = _ui.value
        // A paused sign-in waits for its button: a resume after the cancelled prompt never re-prompts.
        if (signingIn || now == SetupUi.SigningIn || now == SetupUi.SignInPaused || now == SetupUi.SignInTimedOut ||
            now == SetupUi.Done || now == SetupUi.Claiming || (now is SetupUi.Ended && !now.canRetry)
        ) return
        if (checkJob?.isActive == true) return
        val a = attempt
        if (now == SetupUi.Resume || now is SetupUi.Ended || now == SetupUi.ClaimWaiting) _ui.value = storedUi()
        checkJob = step(a) {
            when {
                rec.phase == KeyDevicePhase.CLAIMED && rec.device.claimSecret != null -> awaitCode(a, rec)
                rec.phase == KeyDevicePhase.CLAIMING -> reconcileClaim(a, rec, null)
                else -> askKeyDevice(a, rec, unknown = null, about = null)
            }
        }
    }

    /**
     * "Start again" / "Try again" / "Keep waiting": a claim whose answer is not known is checked
     * (never sent again), a claim waiting for its code goes to the waiting screen, which asks the
     * server; anything else starts this attempt's code over. A key the server may hold is never
     * replaced here.
     */
    fun retry() {
        if (claimInFlight || signingIn) return
        val claim = unresolvedClaim()
        when {
            claim != null -> {
                stop()
                resumeClaim(claim)
            }
            else -> attempt?.entry?.let(::start)
        }
    }

    /**
     * "Yes, set up Kaiku on {server}". The question may have been open for a while: the app adopts
     * the server only if, now, it is still tied to none and holds no record (rule 6); otherwise the
     * link is looked at again with what the phone has become. Undone if the setup is abandoned.
     */
    fun confirmServer() {
        val a = attempt ?: return
        if (_ui.value !is SetupUi.ConfirmServer) return
        stop()
        val before = env.savedServer()
        if (!env.adoptServer(a.origin)) return begin(a.entry, a.origin)
        if (!adoptedServer) {
            replacedServer = before
            adoptedServer = true
        }
        _ui.value = SetupUi.Opening
        step(a) { checkSuitability(a) }
    }

    /**
     * Leaving before anything was claimed: the new key goes and a server this setup adopted is
     * replaced by the one saved before. A recorded claim keeps both.
     */
    private fun abandonUnclaimed() {
        if (keyDevices.record.value != null || keyDevices.unreadable()) return
        attempt?.let(::dropKey)
        if (adoptedServer) {
            env.restoreServer(replacedServer)
            adoptedServer = false
            replacedServer = null
        }
    }

    /** Deletes [a]'s new key if it is still the phone's key and no record names it (rule 4). */
    private fun dropKey(a: Attempt) {
        val mine = a.key ?: return
        a.key = null
        keyDevices.deleteUnclaimedKey(mine.keyId)
    }

    /** Back from the screen-lock settings. */
    fun recheckScreenLock() {
        val a = attempt ?: return
        if (_ui.value !is SetupUi.ScreenLockNeeded) return
        _ui.value = SetupUi.Opening
        step(a) { checkSuitability(a) }
    }

    private suspend fun checkSuitability(a: Attempt) {
        show(a, SetupUi.Opening)
        val d = withContext(keyDispatcher) { suitability.check() }
        live(a)
        when (d) {
            is UvDecision.Unsuitable -> show(
                a,
                when (d.reason) {
                    UnsuitableReason.NO_SCREEN_LOCK -> SetupUi.ScreenLockNeeded(d.reason)
                    UnsuitableReason.NO_HARDWARE_KEYSTORE -> SetupUi.Ended(R.string.setup_cannot_be_key, canRetry = false)
                },
            )
            is UvDecision.Use -> prepareKey(a, d)
        }
    }

    /**
     * The only place a key is made (rule 4), through [KeyDeviceRepository.generateKey]: nothing is
     * made while a record exists (an unresolved claim goes to the waiting screen, a key device stays
     * as it is) or while the store cannot be read. Once begun, the generation runs to its end; then,
     * still without a way to be interrupted, the key is either adopted by [a] (the step is still
     * live) or deleted again.
     */
    private suspend fun makeKey(a: Attempt, d: UvDecision.Use): KeyResult = keyLock.withLock {
        live(a)
        val me = currentCoroutineContext()[Job]
        val made = withContext(NonCancellable) {
            val r = withContext(keyDispatcher) { keyDevices.generateKey(a.nonce, d) }
            if (r is NewKey.Made) {
                if (isLive(a, me)) a.key = r.key else keyDevices.deleteUnclaimedKey(r.key.keyId)
            }
            r
        }
        live(a)
        when (made) {
            is NewKey.Made -> a.key?.let { KeyResult.Made(it) } ?: throw CancellationException("superseded")
            NewKey.RecordExists -> if (unresolvedClaim() != null) KeyResult.ClaimWaiting else KeyResult.AlreadyKey
            NewKey.Unreadable -> KeyResult.Unreadable
        }
    }

    private suspend fun prepareKey(a: Attempt, d: UvDecision.Use) {
        try {
            val k = when (val r = makeKey(a, d)) {
                is KeyResult.Made -> r.key
                KeyResult.ClaimWaiting -> {
                    live(a)
                    _waiting.value = true
                    return show(a, SetupUi.Resume)
                }
                KeyResult.AlreadyKey -> return show(a, SetupUi.Ended(R.string.setup_already_key, canRetry = false))
                KeyResult.Unreadable -> return show(a, SetupUi.Ended(R.string.security_key_storage_unreadable, canRetry = false))
            }
            val p = enrollmentApi.popChallenge(a.entry.tokenB64u, deviceIds.id(), b64u(k.spki), a.origin)
            live(a)
            val signedIn = env.signedInUserId()
            if (signedIn != null && signedIn != p.subject.userId) {
                dropKey(a)
                return show(a, SetupUi.Ended(R.string.setup_for_someone_else, canRetry = false, arg = nameOf(p.subject)))
            }
            val verified = try {
                signer.check(p.challenge, SigKind.ENROLL_POP, p.subject.userId, deviceIds.id())
            } catch (r: ChallengeRejected) {
                logger.warning("PoP challenge refused: ${r.reason}")
                dropKey(a)
                return show(a, SetupUi.Ended(R.string.setup_mismatch, canRetry = false))
            }
            val ctx = verified.context as EnrollPopContext
            if (!ctx.attestationChallenge.contentEquals(a.nonce) ||
                !ctx.newKeyHash.contentEquals(KaikuSig.sha256(k.spki)) ||
                ctx.subjectUsername != p.subject.username
            ) {
                dropKey(a)
                return show(a, SetupUi.Ended(R.string.setup_mismatch, canRetry = false))
            }
            a.pop = p.challenge
            a.subject = p.subject
            a.popEnrollmentId = ctx.enrollmentId
            show(a, SetupUi.ConfirmSubject(p.subject.displayName, ctx.subjectUsername, a.serverName, hostOf(a.origin)))
        } catch (c: CancellationException) {
            throw c
        } catch (x: Exception) {
            live(a)
            dropKey(a)
            show(a, setupErrorFor(x))
        }
    }

    /** "Yes, set up for {name}": UV, the signed claim, then the code (or, with a ticket, done). */
    fun confirmSubject(activity: FragmentActivity) {
        val a = attempt ?: return
        val shown = _ui.value as? SetupUi.ConfirmSubject ?: return
        val ch = a.pop ?: return
        val s = a.subject ?: return
        val k = a.key ?: return
        stop()
        _ui.value = SetupUi.Opening
        step(a) {
            // The prompt names the signed account name and server, never an unsigned display name.
            val prompt = UvPrompt(R.string.security_uv_title, activity.getString(R.string.setup_uv_subtitle, shown.username, shown.host))
            // Signed with this attempt's key or not at all: another key in the alias is never used.
            val out = signer.sign(activity, ch, SigKind.ENROLL_POP, s.userId, deviceIds.id(), Decision.APPROVE, ByteArray(0), prompt, k.keyId)
            live(a)
            when (out) {
                is SignOutcome.Rejected -> {
                    dropKey(a)
                    show(a, SetupUi.Ended(R.string.setup_mismatch, canRetry = false))
                }
                is SignOutcome.Uv -> when (out.result) {
                    UvResult.Cancelled -> show(a, shown)
                    UvResult.LockedOut -> show(a, SetupUi.Ended(R.string.security_locked_out, canRetry = true))
                    else -> {
                        dropKey(a)
                        show(a, SetupUi.Ended(R.string.setup_failed_generic, canRetry = true))
                    }
                }
                is SignOutcome.Signed -> claim(a, ch, out.signatureB64u, s, k)
            }
        }
    }

    /**
     * The signed claim: recorded as `CLAIMING` before it leaves the phone (only where no record is
     * stored and the alias holds this attempt's key), sent once, and settled by its answer through
     * [KeyAnswers.settle]. Nothing is sent without that record: a lost answer, or the app dying,
     * must not cost a key the server may already have enrolled (a self-claim is `done` at once, SM2).
     */
    private suspend fun claim(a: Attempt, ch: ApprovalChallenge, signature: String, s: AccountRef, k: GeneratedKey) {
        live(a)
        val request = EnrollmentClaim(
            setupToken = a.entry.tokenB64u,
            challengeId = ch.challengeId,
            signature = signature,
            publicKey = b64u(k.spki),
            attestationChain = k.attestationChain.map(::b64u),
            label = env.deviceLabel,
            uvClass = k.uvClass.wire,
            apiLevel = env.apiLevel,
        )
        val created = keyDevices.create(
            StoredKeyDevice(
                device = LocalKeyDevice(
                    keyDeviceId = null, userId = s.userId, origin = a.origin, uvClass = k.uvClass,
                    enrollmentId = a.popEnrollmentId ?: "", claimSecret = null, accountName = nameOf(s),
                ),
                phase = KeyDevicePhase.CLAIMING,
                keyId = k.keyId,
                attemptId = a.id,
                // After the challenge the claim answers can no longer be stored; a code it got lasts 5 minutes more.
                claimVoidAfter = parseInstant(ch.expiresAt)?.plus(KeyAnswers.CLAIM_VOID_MARGIN),
            ),
        )
        val rec = (created as? Change.Done)?.record
        if (rec == null) {
            logger.warning("Claim not recorded: ${created.javaClass.simpleName}")
            dropKey(a)
            return show(a, SetupUi.Ended(R.string.setup_failed_generic, canRetry = true))
        }
        a.rec = rec
        // From here until the answer (or the time limit): no link, no Back, no Cancel stops the request.
        show(a, SetupUi.Claiming)
        try {
            val res = withTimeoutOrNull(CLAIM_TIMEOUT_MS) { enrollmentApi.claim(request, ticket = a.entry.ticket, origin = a.origin) }
            live(a)
            if (res == null) {
                logger.warning("Claim answer did not come in time")
                return show(a, SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true))
            }
            if (res.state == EnrollmentState.DONE.wire) return onDone(a, rec, res.authenticatorState, res.readyAt, res.enrollmentId)
            val claimed = rec.copy(
                device = rec.device.copy(enrollmentId = res.enrollmentId, claimSecret = res.claimSecret),
                phase = KeyDevicePhase.CLAIMED,
                claimVoidAfter = null,
            )
            val stored = when (val w = keyDevices.replace(rec, claimed)) {
                is Change.Done -> claimed.also { a.rec = it }
                // The code is shown all the same (it exists only in this answer); storage still says
                // "answer not known", which the next start checks with the server.
                Change.WriteFailed -> claimed
                else -> {
                    logger.warning("Claimed setup not recorded: ${w.javaClass.simpleName}")
                    return show(a, SetupUi.Ended(R.string.setup_failed_generic, canRetry = true))
                }
            }
            show(a, SetupUi.ShowCode(CodeFormat.group(res.code.orEmpty()), parseInstant(res.codeExpiresAt)))
            awaitCode(a, stored)
        } catch (c: CancellationException) {
            throw c
        } catch (x: Exception) {
            live(a)
            val settled = (x as? ApiException)?.let { answers.settle(rec, a.origin, Ask.CLAIM, ServerAnswer.of(it)) }
            when {
                settled?.forgotten == true -> {
                    a.key = null
                    a.rec = null
                    show(a, setupErrorFor(x))
                }
                settled?.verdict == Verdict.CLAIM_UNDECIDED -> reconcileClaim(a, rec, x)
                else -> {
                    logger.warning("Claim answer unknown: ${x.javaClass.simpleName}")
                    val tooMany = x is ApiException && x.status.value == 429
                    show(a, SetupUi.Ended(if (tooMany) R.string.errors_rate_limited else R.string.setup_claim_unsure, canRetry = true))
                }
            }
        }
    }

    /**
     * A claim whose answer did not settle it ([rec] is `CLAIMING`). It is never sent again.
     *
     * With the link still in memory ([a] made this record) the enrolment itself is asked, by a
     * new proof-of-possession challenge for the same link and key, which the server gives under
     * the enrolment's lock: given → the enrolment is still open and every earlier claim of it is
     * void, so the record goes and the same link starts again; "not valid" → the server does not
     * know an open or claimed enrolment for it. Then the key device itself is asked: active or
     * pending → the claim had landed and the setup goes on; none → the record is forgotten if the
     * enrolment is gone too, or once the time a claim could still be stored has passed by the
     * server's clock; until then the screen says it is still checking and asks again by itself.
     */
    private suspend fun reconcileClaim(a: Attempt?, rec: StoredKeyDevice, claimError: Exception?) {
        var unknown: KeyAnswers.EnrolmentUnknown? = null
        var about: Exception? = claimError
        val k = a?.key
        if (a != null && k != null && a.rec?.sameAs(rec) == true) {
            try {
                enrollmentApi.popChallenge(a.entry.tokenB64u, deviceIds.id(), b64u(k.spki), a.origin)
                live(a)
                if (answers.settle(rec, a.origin, Ask.CLAIM_CHECK, ServerAnswer(201)).forgotten) {
                    a.key = null
                    a.rec = null
                    return begin(a.entry, a.origin)
                }
                return show(a, SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true))
            } catch (c: CancellationException) {
                throw c
            } catch (x: ApiException) {
                live(a)
                unknown = (answers.settle(rec, a.origin, Ask.CLAIM_CHECK, ServerAnswer.of(x)) as? KeyAnswers.Settled.Kept)?.unknown
                if (about == null) about = x
            } catch (x: Exception) {
                live(a)
                return show(a, SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true))
            }
        }
        askKeyDevice(a, rec, unknown, about)
    }

    /**
     * The key-device probe for [rec] (no claim secret to ask with, or the enrolment is not known
     * any more): the server's view of this phone's key device decides. [about]: the refusal that
     * led here, for the text when the setup turns out to be over.
     */
    private suspend fun askKeyDevice(a: Attempt?, rec: StoredKeyDevice, unknown: KeyAnswers.EnrolmentUnknown?, about: Exception?) {
        while (true) {
            val st = probe.probe(rec.device.userId, rec.device.origin)
            live(a)
            when (st) {
                ServerKeyState.Active -> return onDone(a, rec, "active", null, null)
                is ServerKeyState.Pending -> return pending(a, rec, st.readyAt)
                ServerKeyState.Unknown -> {
                    if (_ui.value !is SetupUi.Pending) {
                        // On the setup screen: "the connection broke, try again"; on the waiting screen: "no connection".
                        val msg = if (a != null) R.string.setup_claim_unsure else R.string.security_network_error
                        show(a, SetupUi.Ended(msg, canRetry = true))
                    }
                    return
                }
                is ServerKeyState.Gone -> {
                    val settled = answers.settle(rec, originOf(rec), Ask.DEVICE_LOGIN_CHALLENGE, st.answer, unknown = unknown)
                    if (settled.forgotten) {
                        a?.key = null
                        a?.rec = null
                        return show(a, about?.let(::setupErrorFor) ?: SetupUi.Ended(overText(rec), canRetry = false))
                    }
                    // A claim whose answer never came, still within the time the server could store it.
                    val again = (settled as? KeyAnswers.Settled.Kept)?.askAgainIn
                        ?: return show(a, SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true))
                    show(a, SetupUi.ClaimWaiting)
                    delay(again.toMillis() + ASK_AGAIN_SLACK_MS)
                    live(a)
                }
            }
        }
    }

    @StringRes
    private fun overText(rec: StoredKeyDevice): Int =
        if (rec.phase == KeyDevicePhase.CLAIMING) R.string.setup_not_finished else R.string.setup_cancelled

    private fun originOf(rec: StoredKeyDevice): String? = KaikuPayloads.canonicalOrigin(rec.device.origin)

    /**
     * Waits for the code of the claim [start] to be confirmed (the enrolment status long-poll),
     * with the code on screen ([SetupUi.ShowCode]) or, after a restart, without it
     * ([SetupUi.WaitingForCode]). Only the listed answers end the setup; the issuer may still
     * confirm the code through an outage, and the key must then still exist.
     */
    private suspend fun awaitCode(a: Attempt?, start: StoredKeyDevice) {
        val secret = start.device.claimSecret ?: return show(a, SetupUi.Ended(R.string.setup_failed_generic, canRetry = false))
        val enrolment = start.device.enrollmentId
        var backoff = RETRY_MS
        while (_ui.value is SetupUi.ShowCode || _ui.value == SetupUi.WaitingForCode) {
            val showsCode = _ui.value is SetupUi.ShowCode
            val s = try {
                enrollmentApi.status(enrolment, secret, LongPoll.WAIT_SECONDS, start.device.origin).also { backoff = RETRY_MS }
            } catch (c: CancellationException) {
                throw c
            } catch (x: Exception) {
                live(a)
                if (x is ApiException) {
                    val settled = answers.settle(start, originOf(start), Ask.ENROLMENT_STATUS, ServerAnswer.of(x), askedEnrolment = enrolment)
                    // An enrolment the server no longer knows (a finished one is swept 30 days after it
                    // ended): the key device decides whether the setup is over.
                    if (settled.verdict == Verdict.ENROLMENT_UNKNOWN) {
                        return askKeyDevice(a, start, (settled as? KeyAnswers.Settled.Kept)?.unknown, if (showsCode) x else null)
                    }
                }
                // Without the code on screen the next check asks again; with it, this one does.
                if (!showsCode) return
                val retryAfterMs = (x as? ApiException)?.fields?.retryAfterSeconds?.times(1000)
                delay(retryAfterMs ?: backoff)
                live(a)
                backoff = (backoff * 2).coerceAtMost(RETRY_MAX_MS)
                continue
            }
            live(a)
            // Cancelled or reset while the long-poll ran: its answer changes nothing any more.
            if (!(_ui.value is SetupUi.ShowCode || _ui.value == SetupUi.WaitingForCode)) return
            val ended = when (s.state) {
                EnrollmentState.DONE -> return onDone(a, start, s.authenticatorState, s.readyAt, null)
                EnrollmentState.CANCELLED -> R.string.setup_cancelled
                EnrollmentState.EXPIRED -> R.string.setup_expired
                EnrollmentState.FAILED -> R.string.setup_code_failed
                else -> null
            } ?: continue
            val settled = answers.settle(start, originOf(start), Ask.ENROLMENT_STATUS, ServerAnswer(200, state = s.state.wire), askedEnrolment = enrolment)
            if (settled.forgotten) {
                a?.key = null
                a?.rec = null
                return show(a, SetupUi.Ended(ended, canRetry = false))
            }
            return show(a, SetupUi.Ended(R.string.setup_failed_generic, canRetry = true))
        }
    }

    /** The server made this key a key device: active (the final sign-in follows) or pending (the wait). */
    private suspend fun onDone(a: Attempt?, rec: StoredKeyDevice, authenticatorState: String?, readyAt: String?, enrollmentId: String?) {
        live(a)
        val base = if (enrollmentId != null) rec.device.copy(enrollmentId = enrollmentId) else rec.device
        if (authenticatorState == "pending") {
            // Without its ready time (C27 always sends one) the device-login probe tells it.
            val at = parseInstant(readyAt) ?: return askKeyDevice(a, rec, unknown = null, about = null)
            return pending(a, rec, at, base)
        }
        // Active on the server: kept as an active key, so a restart before the sign-in below opens
        // "Confirm it's you" instead of losing the setup. No claim secret is needed any more.
        val next = keyDevices.update(rec) {
            it.copy(device = base.copy(claimSecret = null), phase = KeyDevicePhase.ACTIVE, readyAt = null, claimVoidAfter = null)
        }
        if (next !is Change.Done) return notRecorded(a, next)
        a?.rec = next.record
        show(a, SetupUi.SigningIn)
    }

    private suspend fun pending(a: Attempt?, rec: StoredKeyDevice, readyAt: java.time.Instant, device: LocalKeyDevice = rec.device) {
        live(a)
        val next = keyDevices.update(rec) { it.copy(device = device, phase = KeyDevicePhase.PENDING, readyAt = readyAt, claimVoidAfter = null) }
        if (next !is Change.Done) return notRecorded(a, next)
        a?.rec = next.record
        _waiting.value = true
        show(a, SetupUi.Pending(readyAt))
    }

    /** The server's answer could not be recorded (the record changed, or storage failed): asked again on "Try again". */
    private suspend fun notRecorded(a: Attempt?, c: Change) {
        logger.warning("Setup state not recorded: ${c.javaClass.simpleName}")
        show(a, SetupUi.Ended(R.string.setup_failed_generic, canRetry = true))
    }

    /**
     * The final device-key sign-in (full session, B.4). The screen waits [SIGN_IN_TIMEOUT_MS] at
     * most; a sign-in stopped then finishes what it must (ending a session it made but did not
     * install) on this object's scope, not on the screen's time.
     */
    fun signIn(activity: FragmentActivity) {
        val now = _ui.value
        if ((now != SetupUi.SigningIn && now != SetupUi.SignInPaused && now != SetupUi.SignInTimedOut) || signingIn) return
        signingIn = true
        _ui.value = SetupUi.SigningIn
        // Its own fresh challenge, so a second screen-lock prompt; its text says why.
        val work = scope.async { deviceSignIn.signIn(activity, SignInPurpose.AFTER_SETUP) }
        signInJob = scope.launch {
            val r = try {
                withTimeoutOrNull(SIGN_IN_TIMEOUT_MS) { work.await() }
            } finally {
                signingIn = false
                work.cancel()
            }
            // A sign-in that got through just as the limit hit is a sign-in.
            _ui.value = if (r == null && keyDevices.current() is KeyDeviceState.Active) SetupUi.Done else finalSignInUi(r)
        }
    }

    /**
     * "Cancel" (or Back): before the claim nothing was sent that binds anything and the new key
     * goes. After the claim the server cannot be told; the key and its record stay, so a code typed
     * in anyway still finishes ("Keep waiting" leads to the waiting screen, as does the next start).
     * While the claim itself is on its way nothing is stopped: its answer carries the code.
     */
    fun cancel() {
        if (claimInFlight || signingIn) return
        stop()
        abandonUnclaimed()
        _linkIgnored.value = false
        val claimed = unresolvedClaim() != null
        _ui.value = SetupUi.Ended(if (claimed) R.string.setup_stopped_after_code else R.string.setup_stopped, canRetry = claimed)
    }

    /** Outside a step only (a step uses [show]). */
    private fun end(@StringRes msg: Int) {
        _ui.value = SetupUi.Ended(msg, canRetry = false)
    }
}

private fun b64u(b: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(b)

private fun nameOf(s: AccountRef): String = s.displayName.ifEmpty { s.username }

private fun hostOf(origin: String): String = origin.substringAfter("://")
