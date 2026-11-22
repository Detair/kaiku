package io.wolftown.kaiku.ui.approval

import androidx.annotation.StringRes
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.R
import io.wolftown.kaiku.approval.ChallengeRejected
import io.wolftown.kaiku.approval.RejectReason
import io.wolftown.kaiku.approval.Decision
import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.approval.DeviceSigner
import io.wolftown.kaiku.approval.PcSetupContext
import io.wolftown.kaiku.approval.SigKind
import io.wolftown.kaiku.approval.SignOutcome
import io.wolftown.kaiku.approval.UvPrompt
import io.wolftown.kaiku.approval.UvResult
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.api.LongPoll
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.domain.ScanPayload
import io.wolftown.kaiku.domain.model.ApprovalChallenge
import io.wolftown.kaiku.domain.model.PcSetupPhoneStatus
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.IOException
import java.util.Base64
import java.util.logging.Logger
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

sealed interface PcSetupUi {
    data object Claiming : PcSetupUi
    /** Only fields of the verified, signed context. */
    data class Context(val ctx: PcSetupContext) : PcSetupUi
    /** "I didn't start this" asks "End that computer's session?" before the screen lock. */
    data class ConfirmReport(val ctx: PcSetupContext) : PcSetupUi
    /** The screen lock is being asked, or a step before it runs (the system handles Back). */
    data object Working : PcSetupUi
    /** The signed answer is on its way; bounded by the HTTP timeouts. Back waits for it. */
    data object Sending : PcSetupUi
    /** The unsigned "No" is on its way; it brings nothing back, so Back may leave meanwhile. */
    data object Refusing : PcSetupUi
    /** The six digits for the PC. Held here only: never saved state, disk or a log line. */
    data class Code(val code: String) : PcSetupUi {
        override fun toString() = "Code"
    }
    /** The right code was typed on the PC; the phone has nothing left to show. */
    data object Entered : PcSetupUi
    data class Finished(@StringRes val msg: Int, val ok: Boolean) : PcSetupUi
    data class Error(@StringRes val msg: Int, val rescan: Boolean) : PcSetupUi
}

/**
 * "Set up this PC" on the key phone (spec Steps 2 to 4): claim the scanned token, show the verified
 * context, then "Yes, continue" (UV, signed approve, shows the code), "No" (unsigned refuse) or "I
 * didn't start this" (asks first, then UV and a signed report). While the code is shown the attempt
 * is polled at most once a second. Process death loses the code by design: the person presses
 * "Start again" on the PC.
 */
@HiltViewModel
class PcSetupApprovalViewModel @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val signer: DeviceSigner,
    private val keyDevices: KeyDeviceRepository,
    private val deviceIds: DeviceIdProvider,
    /** Releases outlive the screen: they must reach the server after the ViewModel is cleared. */
    @io.wolftown.kaiku.di.AuthCoroutineScope private val appScope: kotlinx.coroutines.CoroutineScope,
) : ViewModel() {

    private companion object {
        /** The end reasons of C27; any other one reads as the generic failure. */
        private val KNOWN_REASONS = setOf(
            "session_ended", "device_revoked", "no_key_device", "may_approve_off", "account_banned",
            "timeout", "code_expired", "restarted", "cancelled", "refused", "reported",
            "too_many_codes", "enrollment_expired",
        )

        val logger: Logger = Logger.getLogger("PcSetupApproval")
        const val POLL_INTERVAL_MS = 1_000L

        /** The code's lifetime from approval (C27: 5 minutes). */
        const val CODE_LIFETIME_MS = 5 * 60_000L
    }

    private val _ui = MutableStateFlow<PcSetupUi>(PcSetupUi.Claiming)
    val ui: StateFlow<PcSetupUi> = _ui.asStateFlow()

    private var payload: ScanPayload.PcSetup? = null
    private var challenge: ApprovalChallenge? = null
    private var context: PcSetupContext? = null
    private var poll: Job? = null
    /** Ends a code screen whose polling stopped, once the code's time is over. */
    private var codeTimer: Job? = null
    private var pollStopped = false
    private var codeOver = false

    /** A claimed challenge that is not decided yet (to release on leave). */
    private var open = false

    /** True once [start] ran; a recreated screen keeps its claim and its code instead of starting again. */
    val started: Boolean get() = payload != null

    fun start(p: ScanPayload.PcSetup) {
        payload = p
        challenge = null
        open = false
        _ui.value = PcSetupUi.Claiming
        val local = keyDevices.local() ?: run {
            _ui.value = PcSetupUi.Error(R.string.scan_not_a_key, rescan = false)
            return
        }
        viewModelScope.launch {
            val ch = try {
                approvalApi.claimPcSetup(p.tokenB64u)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _ui.value = errorFor(e)
                return@launch
            }
            challenge = ch
            open = true
            try {
                val verified = signer.check(ch, SigKind.PC_SETUP, local.userId, deviceIds.id())
                val ctx = verified.context as PcSetupContext
                context = ctx
                _ui.value = PcSetupUi.Context(ctx)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // ChallengeRejected, or a context that cannot be read: never shown, never signed.
                val reason = (e as? ChallengeRejected)?.reason
                logger.warning("PC setup challenge refused: ${reason ?: e.javaClass.simpleName}")
                releaseQuietly(ch.challengeId)
                _ui.value = PcSetupUi.Error(rejectedText(reason), rescan = true)
            }
        }
    }

    /** "Yes, continue": the screen lock, then the signed approval. */
    fun approve(activity: FragmentActivity) {
        if (_ui.value !is PcSetupUi.Context) return
        decide(activity, Decision.APPROVE)
    }

    /** "I didn't start this": first the question "End that computer's session?". */
    fun askReport() {
        val ctx = context ?: return
        if (_ui.value is PcSetupUi.Context) _ui.value = PcSetupUi.ConfirmReport(ctx)
    }

    fun cancelReport() {
        val ctx = context ?: return
        if (_ui.value is PcSetupUi.ConfirmReport) _ui.value = PcSetupUi.Context(ctx)
    }

    /** "Yes, end it": the screen lock, then the signed report. */
    fun confirmReport(activity: FragmentActivity) {
        if (_ui.value !is PcSetupUi.ConfirmReport) return
        decide(activity, Decision.REPORT)
    }

    private fun decide(activity: FragmentActivity, decision: Decision) {
        val ch = challenge ?: return
        val p = payload ?: return
        val ctx = context ?: return
        val local = keyDevices.local() ?: return
        val back = _ui.value
        _ui.value = PcSetupUi.Working
        viewModelScope.launch {
            val token = try {
                Base64.getUrlDecoder().decode(p.tokenB64u)
            } catch (_: IllegalArgumentException) {
                _ui.value = PcSetupUi.Error(R.string.approval_mismatch, rescan = true)
                return@launch
            }
            val prompt = UvPrompt(
                R.string.security_uv_title,
                activity.getString(if (decision == Decision.APPROVE) R.string.pc_setup_uv_approve else R.string.pc_setup_uv_report),
            )
            when (val out = signer.sign(activity, ch, SigKind.PC_SETUP, local.userId, deviceIds.id(), decision, token, prompt)) {
                is SignOutcome.Rejected -> {
                    releaseQuietly(ch.challengeId)
                    _ui.value = PcSetupUi.Error(rejectedText(out.reason), rescan = true)
                }
                is SignOutcome.Uv -> _ui.value = when (out.result) {
                    UvResult.Cancelled -> back
                    UvResult.LockedOut -> PcSetupUi.Error(R.string.security_locked_out, rescan = false)
                    UvResult.KeyInvalidated -> PcSetupUi.Error(R.string.security_key_invalidated, rescan = false)
                    else -> PcSetupUi.Error(R.string.security_sign_in_failed, rescan = true)
                }
                is SignOutcome.Signed -> try {
                    // Committed: leaving now never releases the claim, and the signed answer still
                    // reaches the server.
                    open = false
                    _ui.value = PcSetupUi.Sending
                    val answer = withContext(NonCancellable) {
                        approvalApi.answerPcSetup(ch.challengeId, decision.wire, p.tokenB64u, out.signatureB64u)
                    }
                    if (decision == Decision.REPORT) {
                        _ui.value = PcSetupUi.Finished(R.string.pc_setup_reported, ok = false)
                    } else {
                        val code = answer.code
                        if (code == null) {
                            _ui.value = PcSetupUi.Error(R.string.pc_setup_failed, rescan = false)
                        } else {
                            _ui.value = PcSetupUi.Code(code)
                            startCodeTimer()
                            startPolling(ctx.attemptId)
                        }
                    }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    _ui.value = answerErrorFor(e, ctx.attemptId)
                }
            }
        }
    }

    /** "No": the unsigned refuse, no prompt. */
    fun deny() {
        val ch = challenge ?: return
        if (_ui.value !is PcSetupUi.Context) return
        // Committed: a "No" is never turned into a release by leaving the screen meanwhile. It
        // brings nothing back, so Back may leave while it is sent.
        open = false
        _ui.value = PcSetupUi.Refusing
        viewModelScope.launch {
            try {
                withContext(NonCancellable) { approvalApi.refusePcSetup(ch.challengeId) }
                _ui.value = PcSetupUi.Finished(R.string.pc_setup_denied, ok = false)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _ui.value = errorFor(e)
            }
        }
    }

    /** System back or leaving the screen: an undecided claim goes back to the PC. */
    private val _leaveAsked = MutableStateFlow(false)
    /** The code screen asked "Leave the code?" after Back. */
    val leaveAsked: StateFlow<Boolean> = _leaveAsked.asStateFlow()

    /**
     * Back: true when the screen may close now. While the code is shown it asks first, because
     * leaving throws the code away and the computer can then only start again.
     */
    fun requestBack(): Boolean {
        // The answer is on its way: Back is swallowed, or the code it brings back would be lost.
        if (_ui.value == PcSetupUi.Sending) return false
        if (_ui.value is PcSetupUi.Code) {
            _leaveAsked.value = true
            return false
        }
        return true
    }

    /** "Stay" on the question: the code stays on screen. */
    fun stay() {
        _leaveAsked.value = false
    }

    fun back() {
        val ch = challenge ?: return
        if (!open) return
        open = false
        appScope.launch { releaseQuietly(ch.challengeId) }
    }

    override fun onCleared() {
        back()
        super.onCleared()
    }

    /**
     * Long-polls the attempt's state while the code is shown (the server answers early on a change;
     * the next request starts a full second after the last one ended). A lost connection changes
     * nothing on screen.
     */
    private fun startPolling(attemptId: String) {
        poll?.cancel()
        poll = viewModelScope.launch {
            var pause = POLL_INTERVAL_MS
            while (true) {
                delay(pause)
                pause = POLL_INTERVAL_MS
                val status = try {
                    approvalApi.pcSetupStatus(attemptId, LongPoll.WAIT_SECONDS)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: ApiException) {
                    when {
                        // Only the server's own "not found" ends the attempt.
                        e.status == HttpStatusCode.NotFound &&
                            e.normalizedCode == "APPROVAL_NOT_FOUND" -> {
                            end(R.string.pc_setup_cancelled)
                            return@launch
                        }
                        // Wait as long as the server asks.
                        e.status == HttpStatusCode.TooManyRequests ->
                            pause = maxOf(POLL_INTERVAL_MS, (e.fields.retryAfterSeconds ?: 0L) * 1_000L)
                        // An answer that says nothing about the attempt: stop asking, but keep
                        // the code on screen until the person leaves or its time is over
                        // (startCodeTimer).
                        e.status.value in 400..499 && e.status != HttpStatusCode.RequestTimeout &&
                            e.status != HttpStatusCode.Unauthorized -> {
                            pollStopped = true
                            endIfCodeOver()
                            return@launch
                        }
                    }
                    continue
                } catch (_: Exception) {
                    continue
                }
                if (apply(status)) return@launch
            }
        }
    }

    /**
     * The code's time: its lifetime of 5 minutes on this phone's own elapsed time since the code was
     * received (never the phone's wall clock, which may be off). Only a screen whose polling stopped
     * ends by it; a polling screen ends with what the server reports.
     */
    private fun startCodeTimer() {
        codeTimer?.cancel()
        pollStopped = false
        codeOver = false
        codeTimer = viewModelScope.launch {
            delay(CODE_LIFETIME_MS)
            codeOver = true
            endIfCodeOver()
        }
    }

    private fun endIfCodeOver() {
        if (pollStopped && codeOver && _ui.value is PcSetupUi.Code) end(R.string.pc_setup_code_expired)
    }

    /** A challenge the phone refused: one that ran out took too long; anything else does not match. */
    private fun rejectedText(reason: RejectReason?): Int =
        if (reason == RejectReason.EXPIRED) R.string.approval_expired else R.string.approval_mismatch

    private fun end(@StringRes msg: Int) {
        _leaveAsked.value = false
        _ui.value = PcSetupUi.Finished(msg, ok = false)
    }

    /** True when the attempt has ended and polling stops. */
    private fun apply(s: PcSetupPhoneStatus): Boolean {
        val end = endText(s)
        if (end != null) {
            _leaveAsked.value = false
            _ui.value = PcSetupUi.Finished(end, ok = s.state == "completed")
            return true
        }
        // The right code was typed: the phone no longer holds it.
        if (s.state == "enrolled") {
            _leaveAsked.value = false
            _ui.value = PcSetupUi.Entered
        }
        return false
    }

    /**
     * The end text of a status, null while the attempt is live. `completed` is success whatever its
     * reason; an unknown state, or an unknown reason on any other state, reads as the generic failure
     * (C27) and ends polling.
     */
    private fun endText(s: PcSetupPhoneStatus): Int? = when {
        // `completed` means the passkey exists, whatever reason comes with it.
        s.state == "completed" -> R.string.pc_setup_done
        s.reason != null && s.reason !in KNOWN_REASONS -> R.string.pc_setup_failed
        else -> knownEndText(s)
    }

    private fun knownEndText(s: PcSetupPhoneStatus): Int? = when (s.state) {
        "waiting", "claimed", "code", "enrolled" -> null
        "completed" -> R.string.pc_setup_done
        "failed" -> R.string.pc_setup_too_many_codes
        "refused" -> R.string.pc_setup_denied
        "reported" -> R.string.pc_setup_reported
        "cancelled", "expired" -> when (s.reason) {
            null, "cancelled", "restarted" ->
                if (s.state == "cancelled") R.string.pc_setup_cancelled else R.string.pc_setup_failed
            "session_ended" -> R.string.pc_setup_session_ended
            "code_expired" -> R.string.pc_setup_code_expired
            "timeout" -> R.string.pc_setup_timeout
            // The passkey on the PC failed (too many, already registered) or never finished: never "Done".
            "enrollment_expired", "too_many_codes", "device_revoked", "no_key_device", "account_banned" ->
                R.string.pc_setup_failed
            "may_approve_off" -> R.string.scan_may_approve_off
            else -> R.string.pc_setup_failed
        }
        else -> R.string.pc_setup_failed
    }

    private suspend fun releaseQuietly(id: String) {
        open = false
        try {
            approvalApi.release(id)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("Release failed: ${e.javaClass.simpleName}")
        }
    }

    /** After a signed answer: a lost response is not guessed at, a dead attempt says why. */
    private suspend fun answerErrorFor(e: Exception, attemptId: String): PcSetupUi {
        if (e is IOException) return PcSetupUi.Error(R.string.pc_setup_answer_lost, rescan = false)
        if (e is ApiException && e.normalizedCode in setOf("APPROVAL_STATE", "APPROVAL_EXPIRED")) {
            val status = try {
                approvalApi.pcSetupStatus(attemptId, waitSeconds = 0)
            } catch (c: CancellationException) {
                throw c
            } catch (_: Exception) {
                null
            }
            val msg = status?.let(::endText)
            // `completed` means the passkey exists: success, as on the poll.
            if (msg != null) return PcSetupUi.Finished(msg, ok = status.state == "completed")
        }
        // The server refused the answer: free the claim so a re-scan is not "already confirming".
        // A lost response (IOException) is not released: the answer may have been taken.
        if (e is ApiException && e.status.value in 400..499) releaseQuietly(challenge?.challengeId ?: return errorFor(e))
        return errorFor(e)
    }

    private fun errorFor(e: Exception): PcSetupUi.Error = when (e) {
        is ApiException -> when (e.normalizedCode) {
            "PC_SETUP_OTHER_ACCOUNT" -> PcSetupUi.Error(R.string.pc_setup_other_account, rescan = false)
            "LOGIN_CODE_STALE" -> PcSetupUi.Error(R.string.pc_setup_code_changed, rescan = true)
            "LOGIN_ATTEMPT_CLAIMED" -> PcSetupUi.Error(R.string.pc_setup_already_claimed, rescan = false)
            "APPROVAL_EXPIRED" -> PcSetupUi.Error(R.string.approval_expired, rescan = true)
            "APPROVAL_NOT_ALLOWED" -> PcSetupUi.Error(R.string.scan_may_approve_off, rescan = false)
            "KEY_DEVICE_REQUIRED" -> PcSetupUi.Error(R.string.scan_not_a_key, rescan = false)
            "APPROVAL_RESPONSE_MISMATCH", "SIGNATURE_INVALID" -> PcSetupUi.Error(R.string.approval_mismatch, rescan = true)
            "TOO_MANY_ATTEMPTS" -> PcSetupUi.Error(R.string.errors_rate_limited, rescan = false)
            else -> PcSetupUi.Error(R.string.pc_setup_failed, rescan = false)
        }
        is IOException -> PcSetupUi.Error(R.string.security_network_error, rescan = true)
        else -> PcSetupUi.Error(R.string.pc_setup_failed, rescan = false)
    }
}
