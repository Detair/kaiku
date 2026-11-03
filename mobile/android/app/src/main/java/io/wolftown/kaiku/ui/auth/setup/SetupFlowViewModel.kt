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
import io.wolftown.kaiku.data.auth.DeviceSignIn
import io.wolftown.kaiku.data.auth.DeviceSignInResult
import io.wolftown.kaiku.data.auth.OriginPin
import io.wolftown.kaiku.data.keydevice.KeyDevicePhase
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.keydevice.KeyDeviceState
import io.wolftown.kaiku.data.keydevice.LocalKeyDevice
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.CodeFormat
import io.wolftown.kaiku.domain.KaikuPayloads
import io.wolftown.kaiku.domain.model.ApprovalChallenge
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
    data class ConfirmSubject(val displayName: String, val username: String, val serverName: String) : SetupUi
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
    data class Ended(@StringRes val msg: Int, val canRetry: Boolean, val arg: String? = null) : SetupUi
}

/** What the setup needs from the app around it (JVM-testable). */
interface SetupEnvironment {
    /** The origin a session or a key ties the app to; null = unconfigured. */
    fun pinnedOrigin(): String?
    fun signedInUserId(): String?
    /** An unconfigured app adopts the setup code's server once the person confirmed it. */
    fun saveServerOrigin(origin: String)
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
    ) : this(enrollmentApi, suitability, keyStore, signer, keyDevices, deviceIds, deviceSignIn, env, Dispatchers.Default)

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
    private var signingIn = false

    val started: Boolean get() = entry != null

    /** For hosts that switch between the scanner and this flow (SetupScanScreen). */
    private val _startedFlow = MutableStateFlow(false)
    val startedFlow: StateFlow<Boolean> = _startedFlow.asStateFlow()

    /** Back to the scanner after an ended or abandoned setup; an unclaimed new key goes. */
    fun reset() {
        if (keyDevices.local() == null && key != null) keyStore.delete()
        entry = null
        key = null
        pop = null
        subject = null
        _startedFlow.value = false
        _ui.value = SetupUi.Opening
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
        _ui.value = SetupUi.Opening
        viewModelScope.launch {
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

    /** "Yes, set up Kaiku on {server}": the unconfigured app adopts this server. */
    fun confirmServer() {
        if (_ui.value !is SetupUi.ConfirmServer) return
        env.saveServerOrigin(origin)
        checkSuitability()
    }

    /** Back from the screen-lock settings. */
    fun recheckScreenLock() {
        if (_ui.value is SetupUi.ScreenLockNeeded) checkSuitability()
    }

    private fun checkSuitability() {
        _ui.value = SetupUi.Opening
        viewModelScope.launch {
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
                if (keyDevices.phase() == KeyDevicePhase.CLAIMED) keyDevices.forget()
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
            _ui.value = SetupUi.ConfirmSubject(p.subject.displayName.ifEmpty { p.subject.username }, ctx.subjectUsername, serverName)
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
        viewModelScope.launch {
            val prompt = UvPrompt(R.string.security_uv_title, activity.getString(R.string.setup_uv_subtitle, shown.displayName))
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
        try {
            val res = enrollmentApi.claim(
                io.wolftown.kaiku.domain.model.EnrollmentClaim(
                    setupToken = e.tokenB64u,
                    challengeId = ch.challengeId,
                    signature = signature,
                    publicKey = b64u(k.spki),
                    attestationChain = k.attestationChain.map(::b64u),
                    label = env.deviceLabel,
                    uvClass = k.uvClass.wire,
                    apiLevel = env.apiLevel,
                ),
                ticket = e.ticket,
                origin = origin,
            )
            val local = LocalKeyDevice(
                keyDeviceId = null, userId = s.userId, origin = origin, uvClass = k.uvClass,
                enrollmentId = res.enrollmentId, claimSecret = res.claimSecret, accountName = nameOf(s),
            )
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
            keyStore.delete()
            fail(x)
        }
    }

    private suspend fun poll(local: LocalKeyDevice) {
        val secret = local.claimSecret ?: return end(R.string.setup_failed_generic)
        var backoff = RETRY_MS
        while (_ui.value is SetupUi.ShowCode) {
            val s = try {
                enrollmentApi.status(local.enrollmentId, secret, 25, origin).also { backoff = RETRY_MS }
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
            val at = parseInstant(readyAt) ?: return end(R.string.setup_failed_generic)
            keyDevices.markPending(local, at)
            _ui.value = SetupUi.Pending(at)
        } else {
            keyDevices.saveClaimed(local)
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
        if (keyDevices.local() == null) keyStore.delete()
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

/** Server refusals during setup (C26) as texts. */
internal fun setupErrorFor(x: Exception): SetupUi.Ended = when (x) {
    is ApiException -> when (x.normalizedCode) {
        "ENROLLMENT_ALREADY_CLAIMED" -> SetupUi.Ended(R.string.setup_already_used, canRetry = false)
        "ENROLLMENT_TOKEN_INVALID" -> SetupUi.Ended(R.string.setup_token_invalid, canRetry = false)
        "DEVICE_UNSUITABLE" -> SetupUi.Ended(R.string.setup_cannot_be_key, canRetry = false)
        "ENROLLMENT_STATE", "APPROVAL_EXPIRED" -> SetupUi.Ended(R.string.setup_expired, canRetry = false)
        "TOO_MANY_ATTEMPTS" -> SetupUi.Ended(R.string.errors_rate_limited, canRetry = true)
        else -> SetupUi.Ended(R.string.setup_failed_generic, canRetry = true)
    }
    is IOException -> SetupUi.Ended(R.string.security_network_error, canRetry = true)
    else -> SetupUi.Ended(R.string.setup_failed_generic, canRetry = true)
}

private fun b64u(b: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(b)

private fun nameOf(s: AccountRef): String = s.displayName.ifEmpty { s.username }

private fun hostOf(origin: String): String = origin.substringAfter("://")

internal fun parseInstant(s: String?): Instant? = s?.let { runCatching { Instant.parse(it) }.getOrNull() }
