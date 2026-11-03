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
import kotlinx.coroutines.delay
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
    /** Active: the screen starts the device-key sign-in (it needs the activity for UV). */
    data object SigningIn : SetupUi
    /** The UV prompt of the final sign-in was cancelled: one button to try again. */
    data object SignInPaused : SetupUi
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

    private var entry: SetupEntry? = null
    private var origin: String = ""
    private var serverName: String = ""
    private var nonce: ByteArray = ByteArray(0)
    private var key: GeneratedKey? = null
    private var pop: ApprovalChallenge? = null
    private var subject: AccountRef? = null
    private var popEnrollmentId: String? = null
    /** The signed claim, kept in memory so an unanswered claim can be sent again unchanged. */
    private var pendingClaim: PendingClaim? = null
    private var signingIn = false
    /** The one running step (open, key, PoP, claim, reconcile): cancel and reset stop it. */
    private var job: kotlinx.coroutines.Job? = null
    /** The server an unconfigured app had saved before this setup adopted the code's one. */
    private var replacedServer: String? = null
    private var adoptedServer = false

    private class PendingClaim(val request: EnrollmentClaim, val ticket: String?) {
        override fun toString() = "PendingClaim"
    }

    val started: Boolean get() = entry != null

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

    /** Back to the scanner after an ended or abandoned setup; an unclaimed new key goes. */
    fun reset() {
        job?.cancel()
        // A claim whose answer is unknown keeps its key and record: the next app start reconciles it.
        abandonUnclaimed()
        entry = null
        key = null
        pop = null
        subject = null
        pendingClaim = null
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

    /** Runs [block] as the one step in flight; a new step or a cancel stops the old one. */
    private fun step(block: suspend () -> Unit) {
        job?.cancel()
        job = viewModelScope.launch { block() }
    }

    /** [start] for a code this flow is not already running (a recreated screen passes the same one). */
    fun startIfNew(e: SetupEntry) {
        if (e != entry) start(e)
    }

    fun start(e: SetupEntry) {
        entry = e
        _startedFlow.value = true
        val o = KaikuPayloads.canonicalOrigin(e.origin) ?: return end(R.string.setup_invalid)
        origin = o
        val pinned = env.pinnedOrigin()
        if (pinned != null && pinned != o) return end(R.string.setup_other_server)
        when (keyDevices.current()) {
            is KeyDeviceState.Active, is KeyDeviceState.Lapsed -> return end(R.string.setup_already_key)
            is KeyDeviceState.Pending -> return end(R.string.setup_already_pending)
            KeyDeviceState.None -> Unit
        }
        if (hasUnresolvedClaim()) {
            _ui.value = SetupUi.Resume
            return
        }
        _ui.value = SetupUi.Opening
        step {
            try {
                val open = enrollmentApi.open(e.tokenB64u, origin)
                serverName = open.serverName
                nonce = Base64.getUrlDecoder().decode(open.enrollmentNonce)
                if (pinned == null && e.source != SetupEntry.Source.TICKET) {
                    _ui.value = SetupUi.ConfirmServer(open.serverName, hostOf(o))
                } else {
                    checkSuitability()
                }
            } catch (c: CancellationException) {
                throw c
            } catch (x: Exception) {
                fail(x)
            }
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
     * "Start again" / "Try again": an unanswered claim is reconciled (resent if the server never
     * got it), a claim waiting for its code goes to the waiting screen, which asks the server;
     * anything else starts over. A key the server may hold is never replaced here.
     */
    fun retry() {
        val local = keyDevices.local()
        when {
            local != null && keyDevices.phase() == KeyDevicePhase.CLAIMING -> reconcileClaim()
            local != null && keyDevices.phase() == KeyDevicePhase.CLAIMED -> _ui.value = SetupUi.Resume
            else -> entry?.let(::start)
        }
    }

    /** "Yes, set up Kaiku on {server}": the unconfigured app adopts this server (undone if abandoned). */
    fun confirmServer() {
        if (_ui.value !is SetupUi.ConfirmServer) return
        if (!adoptedServer) {
            replacedServer = env.savedServer()
            adoptedServer = true
        }
        env.saveServerOrigin(origin)
        checkSuitability()
    }

    /**
     * Leaving before anything was claimed: the new key goes and a server this setup adopted is
     * replaced by the one saved before (Argon part6 #7). A recorded claim keeps both.
     */
    private fun abandonUnclaimed() {
        if (keyDevices.local() != null) return
        // Only the key this instance made, still the one in the store: another setup screen's
        // ViewModel shares the alias and may hold its own unclaimed key (Fable optional).
        val mine = key
        if (mine != null && keyStore.publicKeySpki()?.contentEquals(mine.spki) == true) keyStore.delete()
        key = null
        if (adoptedServer) {
            env.restoreServer(replacedServer)
            adoptedServer = false
            replacedServer = null
        }
    }

    override fun onCleared() {
        job?.cancel()
        abandonUnclaimed()
        super.onCleared()
    }

    /** Back from the screen-lock settings. */
    fun recheckScreenLock() {
        if (_ui.value is SetupUi.ScreenLockNeeded) checkSuitability()
    }

    private fun checkSuitability() {
        _ui.value = SetupUi.Opening
        step {
            when (val d = withContext(keyDispatcher) { suitability.check() }) {
                is UvDecision.Unsuitable -> _ui.value = when (d.reason) {
                    UnsuitableReason.NO_SCREEN_LOCK -> SetupUi.ScreenLockNeeded(d.reason)
                    UnsuitableReason.NO_HARDWARE_KEYSTORE -> SetupUi.Ended(R.string.setup_cannot_be_key, canRetry = false)
                }
                is UvDecision.Use -> prepareKey(d)
            }
        }
    }

    private suspend fun prepareKey(d: UvDecision.Use) {
        val e = entry ?: return
        try {
            // An unfinished earlier setup is replaced explicitly, never silently re-used.
            val k = withContext(keyDispatcher) {
                val phase = keyDevices.phase()
                if (phase == KeyDevicePhase.CLAIMED || phase == KeyDevicePhase.CLAIMING) keyDevices.forget()
                if (keyDevices.local() == null && keyStore.hasKey()) keyStore.delete()
                keyStore.generate(nonce, d)
            }
            key = k
            val p = enrollmentApi.popChallenge(e.tokenB64u, deviceIds.id(), b64u(k.spki), origin)
            val signedIn = env.signedInUserId()
            if (signedIn != null && signedIn != p.subject.userId) {
                keyStore.delete()
                return end(R.string.setup_for_someone_else, arg = nameOf(p.subject))
            }
            val verified = try {
                signer.check(p.challenge, SigKind.ENROLL_POP, p.subject.userId, deviceIds.id())
            } catch (r: ChallengeRejected) {
                logger.warning("PoP challenge refused: ${r.reason}")
                keyStore.delete()
                return end(R.string.setup_mismatch)
            }
            val ctx = verified.context as EnrollPopContext
            if (!ctx.attestationChallenge.contentEquals(nonce) ||
                !ctx.newKeyHash.contentEquals(KaikuSig.sha256(k.spki)) ||
                ctx.subjectUsername != p.subject.username
            ) {
                keyStore.delete()
                return end(R.string.setup_mismatch)
            }
            pop = p.challenge
            subject = p.subject
            popEnrollmentId = ctx.enrollmentId
            _ui.value = SetupUi.ConfirmSubject(p.subject.displayName, ctx.subjectUsername, serverName, hostOf(origin))
        } catch (c: CancellationException) {
            throw c
        } catch (x: Exception) {
            keyStore.delete()
            fail(x)
        }
    }

    /** "Yes, set up for {name}": UV, the signed claim, then the code (or, with a ticket, done). */
    fun confirmSubject(activity: FragmentActivity) {
        val shown = _ui.value as? SetupUi.ConfirmSubject ?: return
        val e = entry ?: return
        val ch = pop ?: return
        val s = subject ?: return
        val k = key ?: return
        _ui.value = SetupUi.Opening
        step {
            // The prompt names the signed account name and server, never an unsigned display name.
            val prompt = UvPrompt(R.string.security_uv_title, activity.getString(R.string.setup_uv_subtitle, shown.username, shown.host))
            when (val out = signer.sign(activity, ch, SigKind.ENROLL_POP, s.userId, deviceIds.id(), Decision.APPROVE, ByteArray(0), prompt)) {
                is SignOutcome.Rejected -> {
                    keyStore.delete()
                    end(R.string.setup_mismatch)
                }
                is SignOutcome.Uv -> when (out.result) {
                    UvResult.Cancelled -> _ui.value = shown
                    UvResult.LockedOut -> _ui.value = SetupUi.Ended(R.string.security_locked_out, canRetry = true)
                    else -> {
                        keyStore.delete()
                        end(R.string.setup_failed_generic, canRetry = true)
                    }
                }
                is SignOutcome.Signed -> claim(e, ch, out.signatureB64u, s, k)
            }
        }
    }

    private suspend fun claim(e: SetupEntry, ch: ApprovalChallenge, signature: String, s: AccountRef, k: GeneratedKey) {
        val request = EnrollmentClaim(
            setupToken = e.tokenB64u,
            challengeId = ch.challengeId,
            signature = signature,
            publicKey = b64u(k.spki),
            attestationChain = k.attestationChain.map(::b64u),
            label = env.deviceLabel,
            uvClass = k.uvClass.wire,
            apiLevel = env.apiLevel,
        )
        pendingClaim = PendingClaim(request, e.ticket)
        // Recorded before the claim leaves the phone: a lost answer (or the app dying) must not cost
        // a key the server may already have enrolled (a self-claim is `done` at once, SM2).
        keyDevices.saveClaiming(
            LocalKeyDevice(
                keyDeviceId = null, userId = s.userId, origin = origin, uvClass = k.uvClass,
                enrollmentId = popEnrollmentId ?: "", claimSecret = null, accountName = nameOf(s),
            ),
        )
        sendClaim()
    }

    /** Sends the kept claim; only a definite refusal deletes the key, anything unclear keeps it. */
    private suspend fun sendClaim() {
        val pc = pendingClaim ?: return
        val claiming = keyDevices.local() ?: return
        try {
            val res = enrollmentApi.claim(pc.request, ticket = pc.ticket, origin = origin)
            pendingClaim = null
            val local = claiming.copy(enrollmentId = res.enrollmentId, claimSecret = res.claimSecret)
            if (res.state == EnrollmentState.DONE.wire) {
                onDone(local, res.authenticatorState, res.readyAt)
            } else {
                keyDevices.saveClaimed(local)
                _ui.value = SetupUi.ShowCode(CodeFormat.group(res.code.orEmpty()), parseInstant(res.codeExpiresAt))
                poll(local)
            }
        } catch (c: CancellationException) {
            throw c
        } catch (x: Exception) {
            if (isDefiniteRefusal(x)) {
                pendingClaim = null
                keyDevices.forget()
                fail(x)
            } else {
                logger.warning("Claim answer unknown: ${x.javaClass.simpleName}")
                val tooMany = x is ApiException && x.status.value == 429
                _ui.value = SetupUi.Ended(if (tooMany) R.string.errors_rate_limited else R.string.setup_claim_unsure, canRetry = true)
            }
        }
    }

    /**
     * After an unanswered claim: the server decides. An active or pending key device goes on
     * (the claim had landed); none known → the kept claim is sent again unchanged (a second
     * claim of a used token is a definite refusal); unreachable → the offer to try again stays.
     */
    private fun reconcileClaim() {
        val local = keyDevices.local() ?: return
        reconcile(local, resend = true)
    }

    private fun reconcile(local: LocalKeyDevice, resend: Boolean) {
        _ui.value = SetupUi.Opening
        step {
            when (val st = probe.probe(local.userId, local.origin)) {
                ServerKeyState.Active -> onDone(local, "active", null)
                is ServerKeyState.Pending -> {
                    keyDevices.markPending(local, st.readyAt)
                    _ui.value = SetupUi.Pending(st.readyAt)
                }
                ServerKeyState.Gone -> if (resend && pendingClaim != null) {
                    sendClaim()
                } else {
                    forgetAndEnd(R.string.setup_not_finished)
                }
                ServerKeyState.Unknown -> _ui.value = SetupUi.Ended(R.string.setup_claim_unsure, canRetry = true)
            }
        }
    }

    private suspend fun poll(local: LocalKeyDevice) {
        val secret = local.claimSecret ?: return end(R.string.setup_failed_generic)
        var backoff = RETRY_MS
        while (_ui.value is SetupUi.ShowCode) {
            val s = try {
                enrollmentApi.status(local.enrollmentId, secret, LongPoll.WAIT_SECONDS, origin).also { backoff = RETRY_MS }
            } catch (c: CancellationException) {
                throw c
            } catch (x: Exception) {
                // Only a gone enrolment ends the setup; the issuer may still confirm the code
                // through an outage, and the key must then still exist.
                if (x is ApiException && (x.status.value == 404 || x.status.value == 410)) {
                    keyDevices.forget()
                    return fail(x)
                }
                val retryAfterMs = (x as? ApiException)?.fields?.retryAfterSeconds?.times(1000)
                delay(retryAfterMs ?: backoff)
                backoff = (backoff * 2).coerceAtMost(RETRY_MAX_MS)
                continue
            }
            // Cancelled or reset while the long-poll ran: its answer changes nothing any more.
            if (_ui.value !is SetupUi.ShowCode) return
            when (s.state) {
                EnrollmentState.DONE -> return onDone(local, s.authenticatorState, s.readyAt)
                EnrollmentState.CANCELLED -> return forgetAndEnd(R.string.setup_cancelled)
                EnrollmentState.EXPIRED -> return forgetAndEnd(R.string.setup_expired)
                EnrollmentState.FAILED -> return forgetAndEnd(R.string.setup_code_failed)
                else -> Unit
            }
        }
    }

    private fun onDone(local: LocalKeyDevice, authenticatorState: String?, readyAt: String?) {
        if (authenticatorState == "pending") {
            // Without its ready time (C27 always sends one) the device-login probe tells it.
            val at = parseInstant(readyAt) ?: return reconcile(local, resend = false)
            keyDevices.markPending(local, at)
            _ui.value = SetupUi.Pending(at)
        } else {
            // Active on the server: kept as an active key, so a restart before the sign-in below
            // opens "Confirm it's you" instead of losing the setup (a self-claim has no claim secret).
            keyDevices.markEnrolled(local)
            _ui.value = SetupUi.SigningIn
        }
    }

    /** The final device-key sign-in (full session, B.4). */
    fun signIn(activity: FragmentActivity) {
        val now = _ui.value
        if ((now != SetupUi.SigningIn && now != SetupUi.SignInPaused) || signingIn) return
        signingIn = true
        _ui.value = SetupUi.SigningIn
        viewModelScope.launch {
            val r = deviceSignIn.signIn(activity)
            signingIn = false
            _ui.value = when (r) {
                DeviceSignInResult.Success -> SetupUi.Done
                DeviceSignInResult.Cancelled -> SetupUi.SignInPaused
                is DeviceSignInResult.Pending -> SetupUi.Pending(r.readyAt)
                else -> SetupUi.Ended(signInResultText(r) ?: R.string.setup_failed_generic, canRetry = false)
            }
        }
    }

    /** "Cancel" before the claim: nothing was sent that binds anything; the new key goes. */
    fun cancel() {
        job?.cancel()
        abandonUnclaimed()
        _ui.value = SetupUi.Ended(R.string.setup_stopped, canRetry = false)
    }

    private fun forgetAndEnd(@StringRes msg: Int) {
        keyDevices.forget()
        end(msg)
    }

    private fun end(@StringRes msg: Int, canRetry: Boolean = false, arg: String? = null) {
        _ui.value = SetupUi.Ended(msg, canRetry, arg)
    }

    private fun fail(x: Exception) {
        _ui.value = setupErrorFor(x)
    }
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
