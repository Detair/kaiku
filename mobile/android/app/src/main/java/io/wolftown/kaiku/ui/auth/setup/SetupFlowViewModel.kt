package io.wolftown.kaiku.ui.auth.setup

import android.os.Build
import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ApprovalKeyStore
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
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.auth.OriginPin
import io.wolftown.kaiku.data.auth.SignInPurpose
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceProbe
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.ServerKeyState
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.CodeFormat
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.EnrollmentClaim
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.domain.model.AccountRef
import io.wolftown.kaiku.ui.auth.signInResultText
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.isActive
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import java.io.IOException
import java.time.Instant
import java.util.Base64
import java.util.logging.Logger
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

sealed interface SetupUi {
    data object Opening : SetupUi
    data class ConfirmServer(val serverName: String, val host: String) : SetupUi
    data class ScreenLockNeeded(val reason: UnsuitableReason) : SetupUi
    /**
     * [username] is the signed `subject_username` and [host] the pinned origin's host: the
     * primary lines (C26: show only signed values); [displayName] is a secondary hint.
     */
    data class ConfirmSubject(val displayName: String, val username: String, val serverName: String, val host: String = "") : SetupUi
    /** [code] already grouped ("123 456"). */
    data class ShowCode(val code: String, val expiresAt: Instant?) : SetupUi
    /** A claim waits for its confirmation after an app restart (the code is never stored). */
    data object WaitingForCode : SetupUi
    data class Pending(val readyAt: Instant) : SetupUi
    /** The signed claim is on its way; its answer carries the code, so nothing interrupts it. */
    data object Claiming : SetupUi
    /** Active: the screen starts the device-key sign-in (it needs the activity for UV). */
    data object SigningIn : SetupUi
    /** The UV prompt of the final sign-in was cancelled: one button to try again. */
    data object SignInPaused : SetupUi
    /** The final sign-in's prompt stayed open past its challenge: say so, one button for a fresh one. */
    data object SignInTimedOut : SetupUi
    data object Done : SetupUi
    /**
     * The setup screen came back without its setup code (process death; the code is
     * kept in memory only) while this phone has a stored setup: the waiting screen takes over.
     */
    data object Resume : SetupUi
    /** The setup screen came back without its setup code before anything was claimed. */
    data object Interrupted : SetupUi
    /** [offerSignIn]: this phone is already a key of the account (C26 `ENROLLMENT_DEVICE_IN_USE`): sign in with it. */
    data class Ended(@StringRes val msg: Int, val canRetry: Boolean, val arg: String? = null, val offerSignIn: Boolean = false) : SetupUi
}

/** What the setup needs from the app around it (JVM-testable). */
interface SetupEnvironment {
    /** The origin a session or a key ties the app to; null = unconfigured. */
    fun pinnedOrigin(): String?
    fun signedInUserId(): String?
    /** An unconfigured app adopts the setup code's server once the person confirmed it. */
    fun saveServerOrigin(origin: String)
    /** The saved server URL (an unconfigured app may have one typed in, without a session). */
    fun savedServer(): String?
    /** Puts back the server an abandoned setup replaced (null: none was saved). */
    fun restoreServer(url: String?)
    val deviceLabel: String
    val apiLevel: Int
}

class AndroidSetupEnvironment @Inject constructor(
    private val originPin: OriginPin,
    private val authState: AuthState,
    private val tokenStorage: TokenStorage,
) : SetupEnvironment {
    override fun pinnedOrigin() = originPin.pinned()
    override fun signedInUserId() = authState.currentUserId.value
    override fun saveServerOrigin(origin: String) {
        if (tokenStorage.getServerUrl()?.let(KaikuPayloads::canonicalOrigin) != origin) tokenStorage.saveServerUrl(origin)
    }
    override fun savedServer(): String? = tokenStorage.getServerUrl()
    override fun restoreServer(url: String?) {
        if (url != null) tokenStorage.saveServerUrl(url)
    }
    override val deviceLabel: String = "${Build.MANUFACTURER} ${Build.MODEL}".trim()
    override val apiLevel: Int = Build.VERSION.SDK_INT
}

/**
 * SM2 on the new phone (F5a/F5c, L2-A1): origin check, open (shows the server
 * name to an unconfigured app), suitability, K_approve with the enrolment nonce
 * as attestation challenge, PoP challenge, the subject question, the signed
 * claim, the code, the status long-poll, then device-key sign-in (full) or the
 * 24-hour wait. With a registration [SetupEntry.ticket] the claim is the app's
 * own (no code).
 *
 * Invariant: one setup link, one server. Every step runs for exactly one [Attempt], the
 * accepted setup code together with its canonical origin, and takes the server only from that
 * attempt, never from a field a later link could change. Whatever changes the flow (a new link,
 * Cancel or Back, "Try again", reset, leaving the screen) first stops the step in flight
 * ([stop]) and only then changes anything; a step checks after every suspension point that it
 * is still the running step of the current attempt ([live]) and otherwise ends without sending a
 * request or changing any state. A link that is unreadable or for another server than the pinned
 * one never touches a setup in progress (it is only noted, [linkIgnored]); while the signed claim
 * or the final sign-in is on its way, no link and no Back interrupts it.
 */
@HiltViewModel
class SetupFlowViewModel internal constructor(
    private val enrollmentApi: EnrollmentApi,
    private val suitability: DeviceSuitabilityChecker,
    private val keyStore: ApprovalKeyStore,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val deviceIds: DeviceIdProvider,
    private val deviceSignIn: DeviceSignIn,
    private val env: SetupEnvironment,
    private val probe: KeyDeviceProbe,
    /** The suitability probe and key generation (StrongBox, attestation) take seconds: never on Main. */
    private val keyDispatcher: CoroutineDispatcher,
) : ViewModel() {

    @Inject
    constructor(
        enrollmentApi: EnrollmentApi,
        suitability: DeviceSuitabilityChecker,
        keyStore: ApprovalKeyStore,
        signer: DeviceSigner,
        keyDevices: KeyDeviceRepository,
        deviceIds: DeviceIdProvider,
        deviceSignIn: DeviceSignIn,
        env: SetupEnvironment,
        probe: KeyDeviceProbe,
    ) : this(enrollmentApi, suitability, keyStore, signer, keyDevices, deviceIds, deviceSignIn, env, probe, Dispatchers.Default)

    private companion object {
        val logger: Logger = Logger.getLogger("SetupFlow")
        const val RETRY_MS = 2_000L
        const val RETRY_MAX_MS = 30_000L
    }

    private val _ui = MutableStateFlow<SetupUi>(SetupUi.Opening)
    val ui: StateFlow<SetupUi> = _ui.asStateFlow()

    /**
     * One accepted setup code and its canonical origin, fixed for good. The rest is filled in by
     * this attempt's own steps only (a superseded step stops before writing, see [live]).
     */
    private class Attempt(val entry: SetupEntry, val origin: String) {
        var serverName: String = ""
        var nonce: ByteArray = ByteArray(0)
        var key: GeneratedKey? = null
        var pop: ApprovalChallenge? = null
        var subject: AccountRef? = null
        var popEnrollmentId: String? = null
        /** The signed claim, kept in memory so an unanswered claim can be sent again unchanged. */
        var pendingClaim: PendingClaim? = null

        override fun toString() = "Attempt"
    }

    private class PendingClaim(val request: EnrollmentClaim, val ticket: String?) {
        override fun toString() = "PendingClaim"
    }

    private var attempt: Attempt? = null
    /** The last code handed to [start] (also one that was refused or not used). */
    private var lastLink: SetupEntry? = null
    private var signingIn = false
    /** The one running step (open, key, PoP, claim, reconcile): [stop] ends it. */
    private var job: Job? = null
    /** Key generation and deletion, one at a time (a superseded step may still be generating). */
    private val keyLock = Mutex()
    /** The server an unconfigured app had saved before this setup adopted the code's one. */
    private var replacedServer: String? = null
    private var adoptedServer = false

    val started: Boolean get() = lastLink != null

    private val _linkIgnored = MutableStateFlow(false)
    /** A setup link arrived while this setup was running and was not used (the screen says so). */
    val linkIgnored: StateFlow<Boolean> = _linkIgnored.asStateFlow()

    private val _scanReady = MutableStateFlow<UvDecision?>(null)
    /** C26 "Suitability": the screen lock and a hardware key store, checked before any scan. */
    val scanReady: StateFlow<UvDecision?> = _scanReady.asStateFlow()

    /** Runs the suitability check before the scanner opens (again after the screen-lock settings). */
    fun checkBeforeScan() {
        viewModelScope.launch { _scanReady.value = withContext(keyDispatcher) { suitability.check() } }
    }

    /** For hosts that switch between the scanner and this flow (SetupScanScreen). */
    private val _startedFlow = MutableStateFlow(false)
    val startedFlow: StateFlow<Boolean> = _startedFlow.asStateFlow()

    /** The signed claim is on its way: nothing may interrupt it (its answer carries the code). */
    private val claimInFlight: Boolean get() = _ui.value == SetupUi.Claiming

    /** Back to the scanner after an ended or abandoned setup; an unclaimed new key goes. */
    fun reset() {
        if (claimInFlight || signingIn) return
        stop()
        // A claim whose answer is unknown keeps its key and record: the next app start reconciles it.
        abandonUnclaimed()
        attempt = null
        lastLink = null
        _linkIgnored.value = false
        _startedFlow.value = false
        _ui.value = SetupUi.Opening
    }

    /**
     * A stored claim (CLAIMING: answer unknown, CLAIMED: waiting for its code) is never replaced by
     * a new key: the waiting screen reconciles it with the server first (Argon part6 #1).
     */
    private fun hasUnresolvedClaim(): Boolean {
        val phase = keyDevices.phase()
        return keyDevices.local() != null && (phase == KeyDevicePhase.CLAIMING || phase == KeyDevicePhase.CLAIMED)
    }

    /** Ends the step in flight; always the first thing before the flow changes. */
    private fun stop() {
        job?.cancel()
        job = null
    }

    /** Runs [block] as the one step in flight for [a]; the step before it is stopped first. */
    private fun step(a: Attempt?, block: suspend () -> Unit) {
        stop()
        job = viewModelScope.launch { if (attempt === a) block() }
    }

    /**
     * After every suspension point of a step: it goes on only while it is still running and [a]
     * is still the current attempt; otherwise it ends here, without a request or a state change.
     */
    private suspend fun live(a: Attempt?) {
        currentCoroutineContext().ensureActive()
        if (attempt !== a) throw CancellationException("superseded")
    }

    /** Sets the screen from a step, only while that step is live. */
    private suspend fun show(a: Attempt?, ui: SetupUi) {
        live(a)
        _ui.value = ui
    }

    /** A running setup a refused link must not disturb; an ended or not yet started one may show the refusal. */
    private fun inProgress(): Boolean {
        val now = _ui.value
        return attempt != null && now !is SetupUi.Ended && now != SetupUi.Interrupted
    }

    /** [start] for a code this flow is not already running (a recreated screen passes the same one). */
    fun startIfNew(e: SetupEntry) {
        if (e != lastLink) start(e)
    }

    fun start(e: SetupEntry) {
        lastLink = e
        _startedFlow.value = true
        // The claim (its answer carries the code) and the final sign-in are never interrupted.
        if (claimInFlight || signingIn) {
            _linkIgnored.value = true
            return
        }
        val o = KaikuPayloads.canonicalOrigin(e.origin)
        val pinned = env.pinnedOrigin()
        val refusal = when {
            o == null -> R.string.setup_invalid
            pinned != null && pinned != o -> R.string.setup_other_server
            else -> null
        }
        if (o == null || refusal != null) {
            // Never redirects or disturbs a setup in progress: that one goes on, untouched.
            if (inProgress()) {
                _linkIgnored.value = true
                return
            }
            stop()
            _ui.value = SetupUi.Ended(refusal ?: R.string.setup_invalid, canRetry = false)
            return
        }
        stop()
        abandonUnclaimed()
        val a = Attempt(e, o)
        attempt = a
        _linkIgnored.value = false
        when (keyDevices.current()) {
            is KeyDeviceState.Active -> return alreadyKey(a)
            is KeyDeviceState.Lapsed -> return startOnLapsedKey(a)
            is KeyDeviceState.Pending -> return end(R.string.setup_already_pending)
            KeyDeviceState.None -> Unit
        }
        if (hasUnresolvedClaim()) {
            _ui.value = SetupUi.Resume
            return
        }
        _ui.value = SetupUi.Opening
        step(a) { open(a) }
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
            start(a.entry)
        }
    }

    /**
     * A signed-out key phone may have lost its key on the server (an admin removed it). The key's
     * own server decides (the public device-login probe): only a key it does not know, or knows as
     * removed, is forgotten and the setup goes on; anything else keeps the key ("already your key").
     * The key is forgotten only if it is still the one that was asked about, still signed out.
     */
    private fun startOnLapsedKey(a: Attempt) {
        val local = keyDevices.local() ?: return end(R.string.setup_already_key)
        _ui.value = SetupUi.Opening
        step(a) {
            val st = probe.probe(local.userId, local.origin)
            live(a)
            if (st != ServerKeyState.Gone) return@step show(a, SetupUi.Ended(R.string.setup_already_key, canRetry = false))
            val now = keyDevices.local()
            if (now != null) {
                if (now != local || keyDevices.current() !is KeyDeviceState.Lapsed) {
                    return@step show(a, SetupUi.Ended(R.string.setup_already_key, canRetry = false))
                }
                keyDevices.forget()
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
        _ui.value = when {
            keyDevices.current() is KeyDeviceState.Active -> SetupUi.Done
            keyDevices.local() != null -> SetupUi.Resume
            else -> SetupUi.Interrupted
        }
    }

    /**
     * "Start again" / "Try again" / "Keep waiting": an unanswered claim is reconciled (resent if the
     * server never got it), a claim waiting for its code goes to the waiting screen, which asks the
     * server; anything else starts this attempt's code over. A key the server may hold is never
     * replaced here.
     */
    fun retry() {
        if (claimInFlight || signingIn) return
        val local = keyDevices.local()
        when {
            local != null && keyDevices.phase() == KeyDevicePhase.CLAIMING -> reconcile(attempt, local, resend = true)
            local != null && keyDevices.phase() == KeyDevicePhase.CLAIMED -> {
                stop()
                _ui.value = SetupUi.Resume
            }
            else -> attempt?.entry?.let(::start)
        }
    }

    /** "Yes, set up Kaiku on {server}": the unconfigured app adopts this server (undone if abandoned). */
    fun confirmServer() {
        val a = attempt ?: return
        if (_ui.value !is SetupUi.ConfirmServer) return
        stop()
        if (!adoptedServer) {
            replacedServer = env.savedServer()
            adoptedServer = true
        }
        env.saveServerOrigin(a.origin)
        _ui.value = SetupUi.Opening
        step(a) { checkSuitability(a) }
    }

    /**
     * Leaving before anything was claimed: the new key goes and a server this setup adopted is
     * replaced by the one saved before (Argon part6 #7). A recorded claim keeps both.
     */
    private fun abandonUnclaimed() {
        if (keyDevices.local() != null) return
        // Only the key this instance made, still the one in the store: another setup screen's
        // ViewModel shares the alias and may hold its own unclaimed key.
        attempt?.let(::dropKey)
        if (adoptedServer) {
            env.restoreServer(replacedServer)
            adoptedServer = false
            replacedServer = null
        }
    }

    /** Deletes [a]'s new key if it is still the stored one and no claim recorded it. */
    private fun dropKey(a: Attempt) {
        val mine = a.key ?: return
        a.key = null
        if (keyDevices.local() == null && keyStore.publicKeySpki()?.contentEquals(mine.spki) == true) keyStore.delete()
    }

    override fun onCleared() {
        stop()
        abandonUnclaimed()
        super.onCleared()
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

    private suspend fun prepareKey(a: Attempt, d: UvDecision.Use) {
        try {
            val k = keyLock.withLock {
                live(a)
                // An unfinished earlier setup is replaced explicitly, never silently re-used. The work
                // runs to its end once begun; a key made for an attempt that was replaced meanwhile goes.
                val g = withContext(keyDispatcher + NonCancellable) {
                    val phase = keyDevices.phase()
                    if (phase == KeyDevicePhase.CLAIMED || phase == KeyDevicePhase.CLAIMING) keyDevices.forget()
                    if (keyDevices.local() == null && keyStore.hasKey()) keyStore.delete()
                    keyStore.generate(a.nonce, d)
                }
                a.key = g
                if (attempt !== a || !currentCoroutineContext().isActive) {
                    dropKey(a)
                    throw CancellationException("superseded")
                }
                g
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
            val out = signer.sign(activity, ch, SigKind.ENROLL_POP, s.userId, deviceIds.id(), Decision.APPROVE, ByteArray(0), prompt)
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
        a.pendingClaim = PendingClaim(request, a.entry.ticket)
        // Recorded before the claim leaves the phone: a lost answer (or the app dying) must not cost
        // a key the server may already have enrolled (a self-claim is `done` at once, SM2).
        keyDevices.saveClaiming(
            LocalKeyDevice(
                keyDeviceId = null, userId = s.userId, origin = a.origin, uvClass = k.uvClass,
                enrollmentId = a.popEnrollmentId ?: "", claimSecret = null, accountName = nameOf(s),
            ),
        )
        sendClaim(a)
    }

    /** Sends the kept claim; only a definite refusal deletes the key, anything unclear keeps it. */
    private suspend fun sendClaim(a: Attempt) {
        val pc = a.pendingClaim ?: return
        val claiming = keyDevices.local() ?: return
        // From here until the answer: no link, no Back, no Cancel stops the request.
        show(a, SetupUi.Claiming)
        try {
            val res = enrollmentApi.claim(pc.request, ticket = pc.ticket, origin = a.origin)
            live(a)
            a.pendingClaim = null
            val local = claiming.copy(enrollmentId = res.enrollmentId, claimSecret = res.claimSecret)
            if (res.state == EnrollmentState.DONE.wire) {
                onDone(a, local, res.authenticatorState, res.readyAt)
            } else {
                keyDevices.saveClaimed(local)
                show(a, SetupUi.ShowCode(CodeFormat.group(res.code.orEmpty()), parseInstant(res.codeExpiresAt)))
                poll(a, local)
            }
        } catch (c: CancellationException) {
            throw c
        } catch (x: Exception) {
            live(a)
            if (isDefiniteRefusal(x)) {
                a.pendingClaim = null
                keyDevices.forget()
                show(a, setupErrorFor(x))
            } else {
                logger.warning("Claim answer unknown: ${x.javaClass.simpleName}")
                val tooMany = x is ApiException && x.status.value == 429
                show(a, SetupUi.Ended(if (tooMany) R.string.errors_rate_limited else R.string.setup_claim_unsure, canRetry = true))
            }
        }
    }

    /**
     * After an unanswered claim: the server decides. An active or pending key device goes on
     * (the claim had landed); none known → the kept claim is sent again unchanged (a second
     * claim of a used token is a definite refusal); unreachable → the offer to try again stays.
     */
    private fun reconcile(a: Attempt?, local: LocalKeyDevice, resend: Boolean) {
        _ui.value = SetupUi.Opening
        step(a) {
            val st = probe.probe(local.userId, local.origin)
            live(a)
            when (st) {
                ServerKeyState.Active -> onDone(a, local, "active", null)
                is ServerKeyState.Pending -> {
                    keyDevices.markPending(local, st.readyAt)
                    show(a, SetupUi.Pending(st.readyAt))
                }
                ServerKeyState.Gone -> if (resend && a?.pendingClaim != null) {
                    sendClaim(a)
                } else {
                    keyDevices.forget()
                    show(a, SetupUi.Ended(R.string.setup_not_finished, canRetry = false))
                }
                ServerKeyState.Unknown -> show(a, SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true))
            }
        }
    }

    private suspend fun poll(a: Attempt, local: LocalKeyDevice) {
        val secret = local.claimSecret ?: return show(a, SetupUi.Ended(R.string.setup_failed_generic, canRetry = false))
        var backoff = RETRY_MS
        while (_ui.value is SetupUi.ShowCode) {
            val s = try {
                enrollmentApi.status(local.enrollmentId, secret, LongPoll.WAIT_SECONDS, a.origin).also { backoff = RETRY_MS }
            } catch (c: CancellationException) {
                throw c
            } catch (x: Exception) {
                live(a)
                // Only a gone enrolment ends the setup; the issuer may still confirm the code
                // through an outage, and the key must then still exist.
                if (x is ApiException && (x.status.value == 404 || x.status.value == 410)) {
                    return afterGoneEnrolment(a, local, x)
                }
                val retryAfterMs = (x as? ApiException)?.fields?.retryAfterSeconds?.times(1000)
                delay(retryAfterMs ?: backoff)
                backoff = (backoff * 2).coerceAtMost(RETRY_MAX_MS)
                continue
            }
            live(a)
            // Cancelled or reset while the long-poll ran: its answer changes nothing any more.
            if (_ui.value !is SetupUi.ShowCode) return
            when (s.state) {
                EnrollmentState.DONE -> return onDone(a, local, s.authenticatorState, s.readyAt)
                EnrollmentState.CANCELLED -> return forgetAndEnd(a, R.string.setup_cancelled)
                EnrollmentState.EXPIRED -> return forgetAndEnd(a, R.string.setup_expired)
                EnrollmentState.FAILED -> return forgetAndEnd(a, R.string.setup_code_failed)
                else -> Unit
            }
        }
    }

    /**
     * The enrolment status answered 404/410. A finished enrolment is swept 30 days after it
     * ended and then answers 404 too, so the key device decides (the device-login probe): only
     * a key the server does not know is forgotten; unreachable keeps it for a retry.
     */
    private suspend fun afterGoneEnrolment(a: Attempt, local: LocalKeyDevice, x: Exception) {
        val st = probe.probe(local.userId, local.origin)
        live(a)
        when (st) {
            ServerKeyState.Active -> onDone(a, local, "active", null)
            is ServerKeyState.Pending -> {
                keyDevices.markPending(local, st.readyAt)
                show(a, SetupUi.Pending(st.readyAt))
            }
            ServerKeyState.Gone -> {
                keyDevices.forget()
                show(a, setupErrorFor(x))
            }
            ServerKeyState.Unknown -> show(a, SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true))
        }
    }

    private suspend fun onDone(a: Attempt?, local: LocalKeyDevice, authenticatorState: String?, readyAt: String?) {
        live(a)
        if (authenticatorState == "pending") {
            // Without its ready time (C27 always sends one) the device-login probe tells it.
            val at = parseInstant(readyAt) ?: return reconcile(a, local, resend = false)
            keyDevices.markPending(local, at)
            show(a, SetupUi.Pending(at))
        } else {
            // Active on the server: kept as an active key, so a restart before the sign-in below
            // opens "Confirm it's you" instead of losing the setup (a self-claim has no claim secret).
            keyDevices.markEnrolled(local)
            show(a, SetupUi.SigningIn)
        }
    }

    /** The final device-key sign-in (full session, B.4). */
    fun signIn(activity: FragmentActivity) {
        val now = _ui.value
        if ((now != SetupUi.SigningIn && now != SetupUi.SignInPaused && now != SetupUi.SignInTimedOut) || signingIn) return
        signingIn = true
        _ui.value = SetupUi.SigningIn
        viewModelScope.launch {
            // Its own fresh challenge, so a second screen-lock prompt; its text says why.
            val r = try {
                deviceSignIn.signIn(activity, SignInPurpose.AFTER_SETUP)
            } finally {
                signingIn = false
            }
            _ui.value = when (r) {
                DeviceSignInResult.Success -> SetupUi.Done
                DeviceSignInResult.Cancelled -> SetupUi.SignInPaused
                DeviceSignInResult.Expired -> SetupUi.SignInTimedOut
                is DeviceSignInResult.Pending -> SetupUi.Pending(r.readyAt)
                else -> SetupUi.Ended(signInResultText(r) ?: R.string.setup_failed_generic, canRetry = false)
            }
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
        val claimed = hasUnresolvedClaim()
        _ui.value = SetupUi.Ended(if (claimed) R.string.setup_stopped_after_code else R.string.setup_stopped, canRetry = claimed)
    }

    private suspend fun forgetAndEnd(a: Attempt, @StringRes msg: Int) {
        live(a)
        keyDevices.forget()
        show(a, SetupUi.Ended(msg, canRetry = false))
    }

    /** Outside a step only (a step uses [show]). */
    private fun end(@StringRes msg: Int) {
        _ui.value = SetupUi.Ended(msg, canRetry = false)
    }
}

/** What the Back gesture does on a setup step. */
enum class SetupBack {
    /** A question before anything binds: the same as its "Cancel". */
    CANCEL,
    /** A question about the server or the account, or the code is out (or awaited): ask before leaving. */
    ASK,
    /** A prompt or the final sign-in runs: Back does nothing. */
    STAY,
    /** An ended step: leave as usual. */
    LEAVE,
}

internal fun setupBackAction(ui: SetupUi): SetupBack = when (ui) {
    is SetupUi.ShowCode, SetupUi.WaitingForCode, is SetupUi.ConfirmServer, is SetupUi.ConfirmSubject -> SetupBack.ASK
    SetupUi.Opening, is SetupUi.ScreenLockNeeded -> SetupBack.CANCEL
    SetupUi.Claiming, SetupUi.SigningIn, SetupUi.Done -> SetupBack.STAY
    else -> SetupBack.LEAVE
}

/**
 * A claim the server definitely refused (it answered and nothing was enrolled): a 4xx other
 * than a timeout or rate limit. Network errors, 5xx and unreadable answers are unclear: the
 * claim may have landed.
 */
private fun isDefiniteRefusal(x: Exception): Boolean =
    x is ApiException && x.status.value in 400..499 && x.status.value != 408 && x.status.value != 429

/** Server refusals during setup (C26) as texts. */
internal fun setupErrorFor(x: Exception): SetupUi.Ended = when (x) {
    is ApiException -> when (x.normalizedCode) {
        "ENROLLMENT_ALREADY_CLAIMED" -> SetupUi.Ended(R.string.setup_already_used, canRetry = false)
        "ENROLLMENT_TOKEN_INVALID" -> SetupUi.Ended(R.string.setup_token_invalid, canRetry = false)
        "DEVICE_UNSUITABLE" -> SetupUi.Ended(R.string.setup_cannot_be_key, canRetry = false)
        "ENROLLMENT_STATE", "APPROVAL_EXPIRED" -> SetupUi.Ended(R.string.setup_expired, canRetry = false)
        "TOO_MANY_ATTEMPTS" -> SetupUi.Ended(R.string.errors_rate_limited, canRetry = true)
        // C26/C27 amendment (2026-11-03): nothing was revoked; this phone already is that account's key.
        "ENROLLMENT_DEVICE_IN_USE" -> SetupUi.Ended(R.string.setup_device_in_use, canRetry = false, offerSignIn = true)
        "REGISTRATION_TICKET_INVALID" -> SetupUi.Ended(R.string.setup_registration_expired, canRetry = false)
        "ACCOUNT_REQUEST_NOT_READY" -> SetupUi.Ended(
            if (x.fields.state == "expired") R.string.setup_request_expired else R.string.setup_request_not_ready,
            canRetry = false,
        )
        else -> SetupUi.Ended(R.string.setup_failed_generic, canRetry = true)
    }
    is IOException -> SetupUi.Ended(R.string.security_network_error, canRetry = true)
    else -> SetupUi.Ended(R.string.setup_failed_generic, canRetry = true)
}

private fun b64u(b: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(b)

private fun nameOf(s: AccountRef): String = s.displayName.ifEmpty { s.username }

private fun hostOf(origin: String): String = origin.substringAfter("://")

internal fun parseInstant(s: String?): Instant? = s?.let { runCatching { Instant.parse(it) }.getOrNull() }
