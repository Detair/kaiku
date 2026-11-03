package io.wolftown.kaiku.ui.approval

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ChallengeRejected
import io.wolftown.kaiku.approval.Decision
import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.DeviceStepUp
import io.wolftown.kaiku.approval.EnrollContext
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.SignOutcome
import io.wolftown.kaiku.approval.StepUpOutcome
import io.wolftown.kaiku.approval.UvPrompt
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.api.EnrollmentApi
import io.wolftown.kaiku.data.auth.AccountFlags
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.ws.RealtimeSocket
import io.wolftown.kaiku.data.ws.ServerEvent
import io.wolftown.kaiku.domain.CodeFormat
import io.wolftown.kaiku.domain.model.Enrollment
import io.wolftown.kaiku.domain.model.EnrollmentIssued
import io.wolftown.kaiku.domain.model.EnrollmentState
import io.wolftown.kaiku.domain.model.StepUpAction
import io.wolftown.kaiku.ui.auth.setup.parseInstant
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
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

/** Who the setup code is for: this account's next phone, or (admin) someone else's. */
sealed interface IssuerMode {
    data object Own : IssuerMode
    data class Admin(
        val userId: String,
        val displayName: String,
        val mode: String,
        val claimantMethod: String,
        val note: String,
        val lostDeviceIds: List<String>,
        val newAccount: Boolean,
    ) : IssuerMode
}

sealed interface IssuerUi {
    data object Starting : IssuerUi
    data class ShowSetup(val setupUrl: String, val expiresAt: Instant?) : IssuerUi {
        override fun toString() = "ShowSetup(expiresAt=$expiresAt)"
    }
    /** "A device calling itself '[label]' wants to become your key"; [wrong] after a wrong code. */
    data class TypeCode(val label: String, val digits: String, val triesLeft: Int, val wrong: Boolean = false) : IssuerUi {
        override fun toString() = "TypeCode(label=$label, triesLeft=$triesLeft, wrong=$wrong)"
    }
    data object Confirming : IssuerUi
    /**
     * [waits]: an existing account's new phone waits for its admin account request (F5c, SM3);
     * [readyAt] is that request's `ready_at` from the server, null while unknown.
     */
    data class Done(val label: String, val waits: Boolean = false, val readyAt: Instant? = null) : IssuerUi
    data class Ended(@StringRes val msg: Int) : IssuerUi
}

/** F5a: only a key device whose account may approve offers "Add a phone" (Q4). */
fun canAddPhone(mayApprove: Boolean?, isKeyDevice: Boolean): Boolean = mayApprove == true && isKeyDevice

/**
 * The issuer side of SM2 on a key phone (F5a own, F5c admin): step-up, issue,
 * show the setup code, learn the claim (WS hint or a 3 s poll), take the 6
 * digits the new phone shows on a keypad, and sign the `enroll` approval with
 * them. A wrong code keeps the keypad with the tries left; the third ends it.
 */
@HiltViewModel
class EnrollmentIssuerViewModel internal constructor(
    private val stepUp: DeviceStepUp,
    private val enrollmentApi: EnrollmentApi,
    private val approvalApi: ApprovalApi,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val deviceIds: DeviceIdProvider,
    private val mayApprove: suspend () -> Boolean?,
    private val events: Flow<ServerEvent>,
    private val pollMs: Long = 3_000L,
    private val issueAdmin: (suspend (IssuerMode.Admin) -> EnrollmentIssued)? = null,
    /** Cancel and deny outlive the screen (Back pops it at once). */
    private val appScope: kotlinx.coroutines.CoroutineScope? = null,
    /** The admin request's authoritative `ready_at` after an existing account's setup (C27). */
    private val readyAtFor: (suspend (IssuerMode.Admin, Enrollment?) -> Instant?)? = null,
) : ViewModel() {

    @Inject
    constructor(
        stepUp: DeviceStepUp,
        enrollmentApi: EnrollmentApi,
        approvalApi: ApprovalApi,
        signer: DeviceSigner,
        keyDevices: KeyDeviceRepository,
        deviceIds: DeviceIdProvider,
        flags: AccountFlags,
        socket: RealtimeSocket,
        adminApi: io.wolftown.kaiku.data.api.AdminApi,
        @io.wolftown.kaiku.di.AuthCoroutineScope appScope: kotlinx.coroutines.CoroutineScope,
    ) : this(
        stepUp, enrollmentApi, approvalApi, signer, keyDevices, deviceIds,
        mayApprove = { flags.refresh(); flags.mayApprove() },
        events = socket.events,
        issueAdmin = { m -> adminApi.issueEnrollment(m.userId, m.mode, m.claimantMethod, m.note, m.lostDeviceIds) },
        appScope = appScope,
        readyAtFor = { m, e ->
            // C27 admin user view: `open_request` is the admin request this setup started.
            adminApi.user(m.userId).openRequest
                ?.takeIf { r -> e?.requestId == null || r.id == e.requestId }
                ?.readyAt?.let(::parseInstant)
        },
    )

    private companion object {
        val logger: Logger = Logger.getLogger("EnrollmentIssuer")
        const val SETUP_TOKEN_MS = 10 * 60 * 1000L
    }

    private val _ui = MutableStateFlow<IssuerUi>(IssuerUi.Starting)
    val ui: StateFlow<IssuerUi> = _ui.asStateFlow()

    private var mode: IssuerMode = IssuerMode.Own
    private var enrollmentId: String? = null
    private var openChallengeId: String? = null
    private var watch: Job? = null

    fun start(activity: FragmentActivity, m: IssuerMode) {
        if (_ui.value != IssuerUi.Starting && _ui.value !is IssuerUi.Ended) return
        mode = m
        _ui.value = IssuerUi.Starting
        viewModelScope.launch {
            if (m == IssuerMode.Own && mayApprove() == false) {
                _ui.value = IssuerUi.Ended(R.string.scan_may_approve_off)
                return@launch
            }
            val (action, target, label) = when (m) {
                IssuerMode.Own -> Triple(StepUpAction.ENROLLMENT_ISSUE, "", activity.getString(R.string.security_step_up_enrollment_issue))
                is IssuerMode.Admin -> Triple(StepUpAction.ADMIN_ENROLLMENT_ISSUE, m.userId, m.displayName)
            }
            when (val out = stepUp.stepUp(activity, action, target, label)) {
                is StepUpOutcome.Granted -> Unit
                StepUpOutcome.Cancelled -> return@launch end(R.string.issuer_stopped)
                StepUpOutcome.NotKeyDevice -> return@launch end(R.string.issuer_key_phone_required)
                StepUpOutcome.ProbationSession -> return@launch end(R.string.security_probation_read_only)
                is StepUpOutcome.Failed -> {
                    logger.info("Step-up failed: ${out.code}")
                    return@launch end(R.string.security_sign_in_failed)
                }
            }
            try {
                val issued = when (m) {
                    IssuerMode.Own -> enrollmentApi.issueOwn()
                    is IssuerMode.Admin -> issueAdmin?.invoke(m) ?: return@launch end(R.string.security_sign_in_failed)
                }
                enrollmentId = issued.enrollmentId
                _ui.value = IssuerUi.ShowSetup(issued.setupUrl, parseInstant(issued.expiresAt))
                watchForClaim(issued.enrollmentId)
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                _ui.value = IssuerUi.Ended(issuerErrorFor(e))
            }
        }
    }

    private fun watchForClaim(id: String) {
        watch?.cancel()
        watch = viewModelScope.launch {
            launch {
                events.collect { e -> if (e is ServerEvent.EnrollmentUpdated && e.enrollmentId == id) refetch(id) }
            }
            // Bounded by the setup token's lifetime (10 min); then the code is useless anyway.
            repeat((SETUP_TOKEN_MS / pollMs).toInt() + 1) {
                if (_ui.value !is IssuerUi.ShowSetup) return@launch
                delay(pollMs)
                refetch(id)
            }
            if (_ui.value is IssuerUi.ShowSetup) end(R.string.issuer_expired)
        }
    }

    private suspend fun refetch(id: String) {
        if (_ui.value !is IssuerUi.ShowSetup) return
        val e = try {
            enrollmentApi.get(id)
        } catch (c: CancellationException) {
            throw c
        } catch (_: IOException) {
            return
        } catch (x: ApiException) {
            return end(issuerErrorFor(x))
        }
        onEnrollment(e)
    }

    private fun onEnrollment(e: Enrollment) {
        when (e.state) {
            EnrollmentState.CLAIMED -> {
                watch?.cancel()
                _ui.value = IssuerUi.TypeCode(e.claimed?.label.orEmpty(), "", e.triesLeft)
            }
            EnrollmentState.CANCELLED -> end(R.string.setup_cancelled)
            EnrollmentState.EXPIRED -> end(R.string.issuer_expired)
            EnrollmentState.FAILED -> end(R.string.issuer_three_wrong)
            else -> Unit
        }
    }

    fun onDigits(v: String) {
        val t = _ui.value as? IssuerUi.TypeCode ?: return
        _ui.value = t.copy(digits = CodeFormat.sanitize(v))
    }

    /** "Confirm": never automatic, only with 6 digits. A fresh challenge every time. */
    fun confirmCode(activity: FragmentActivity) {
        val t = _ui.value as? IssuerUi.TypeCode ?: return
        val id = enrollmentId ?: return
        val local = keyDevices.local() ?: return end(R.string.issuer_key_phone_required)
        if (!CodeFormat.isComplete(t.digits)) return
        _ui.value = IssuerUi.Confirming
        viewModelScope.launch {
            try {
                val ch = approvalApi.enrollmentChallenge(id)
                openChallengeId = ch.challengeId
                val verified = try {
                    signer.check(ch, SigKind.ENROLL, local.userId, deviceIds.id())
                } catch (r: ChallengeRejected) {
                    logger.warning("Enrol challenge refused: ${r.reason}")
                    return@launch mismatch(ch.challengeId)
                }
                val ctx = verified.context as EnrollContext
                if (ctx.enrollmentId != id || ctx.label != t.label || !subjectMatches(ctx, local.userId)) {
                    return@launch mismatch(ch.challengeId)
                }
                val digits = t.digits.toByteArray(Charsets.US_ASCII)
                val prompt = UvPrompt(R.string.security_uv_title, activity.getString(R.string.issuer_uv_subtitle, ctx.label, ctx.subjectUsername))
                when (val out = signer.sign(activity, ch, SigKind.ENROLL, local.userId, deviceIds.id(), Decision.APPROVE, digits, prompt)) {
                    is SignOutcome.Rejected -> mismatch(ch.challengeId)
                    is SignOutcome.Uv -> _ui.value = when (out.result) {
                        UvResult.Cancelled -> t
                        UvResult.LockedOut -> IssuerUi.Ended(R.string.security_locked_out)
                        UvResult.KeyInvalidated -> IssuerUi.Ended(R.string.security_key_invalidated)
                        else -> t
                    }
                    is SignOutcome.Signed -> {
                        try {
                            val r = approvalApi.answer(ch.challengeId, Decision.APPROVE.wire, b64u(digits), out.signatureB64u)
                            openChallengeId = null
                            _ui.value = doneFor(t.label, r.enrollment)
                        } catch (x: ApiException) {
                            openChallengeId = null
                            when (x.normalizedCode) {
                                "ENROLL_CODE_WRONG" -> _ui.value = t.copy(digits = "", triesLeft = x.fields.triesLeft ?: (t.triesLeft - 1), wrong = true)
                                else -> end(issuerErrorFor(x))
                            }
                        }
                    }
                }
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                end(issuerErrorFor(e))
            }
        }
    }

    private fun subjectMatches(ctx: EnrollContext, me: String): Boolean = when (val m = mode) {
        IssuerMode.Own -> ctx.subjectId == me
        is IssuerMode.Admin -> ctx.subjectId == m.userId
    }

    /**
     * F5c: an existing account waits for its admin request (24–72 h, `ACCOUNT_REQUEST_DELAY_HOURS`),
     * shown with the request's own `ready_at` (never a time the phone made up); without it the
     * screen says that it waits. A new account is active at once; own phones too.
     */
    private suspend fun doneFor(label: String, e: Enrollment?): IssuerUi.Done {
        val m = mode as? IssuerMode.Admin ?: return IssuerUi.Done(label)
        if (m.newAccount) return IssuerUi.Done(label)
        val at = try {
            readyAtFor?.invoke(m, e)
        } catch (c: CancellationException) {
            throw c
        } catch (x: Exception) {
            logger.info("Ready time not known: ${x.javaClass.simpleName}")
            null
        }
        return IssuerUi.Done(label, waits = true, readyAt = at)
    }

    private suspend fun mismatch(challengeId: String) {
        try {
            approvalApi.deny(challengeId)
        } catch (c: CancellationException) {
            throw c
        } catch (_: Exception) {
        }
        openChallengeId = null
        end(R.string.approval_mismatch)
    }

    /** "Cancel": the unsigned deny while a challenge is open, else the DELETE of the enrolment. */
    fun cancel() {
        val id = enrollmentId
        val ch = openChallengeId
        watch?.cancel()
        _ui.value = IssuerUi.Ended(R.string.issuer_stopped)
        if (id == null) return
        (appScope ?: viewModelScope).launch {
            try {
                if (ch != null) approvalApi.deny(ch) else enrollmentApi.cancel(id)
            } catch (c: CancellationException) {
                throw c
            } catch (e: Exception) {
                logger.info("Cancel failed: ${e.javaClass.simpleName}")
            }
        }
    }

    private fun end(@StringRes msg: Int) {
        watch?.cancel()
        _ui.value = IssuerUi.Ended(msg)
    }
}

@StringRes
internal fun issuerErrorFor(e: Exception): Int = when (e) {
    is ApiException -> when (e.normalizedCode) {
        "ENROLLMENT_FAILED" -> R.string.issuer_three_wrong
        "ENROLLMENT_LIMIT" -> R.string.issuer_limit
        "KEY_DEVICE_REQUIRED" -> R.string.issuer_key_phone_required
        "APPROVAL_NOT_ALLOWED" -> R.string.scan_may_approve_off
        "ENROLLMENT_STATE", "APPROVAL_EXPIRED" -> R.string.issuer_expired
        "STEP_UP_REQUIRED" -> R.string.security_sign_in_failed
        "ACCOUNT_REQUEST_OPEN" -> R.string.admin_request_open
        "NO_INDEPENDENT_CANCELLER" -> R.string.admin_no_canceller
        "ENROLLMENT_TARGET_SELF" -> R.string.admin_target_self
        else -> R.string.security_sign_in_failed
    }
    is IOException -> R.string.security_network_error
    else -> R.string.security_sign_in_failed
}

private fun b64u(b: ByteArray): String = Base64.getUrlEncoder().withoutPadding().encodeToString(b)
