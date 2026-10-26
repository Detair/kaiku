package io.wolftown.kaiku.push

import android.content.Context
import androidx.hilt.work.HiltWorker
import androidx.work.CoroutineWorker
import androidx.work.Data
import androidx.work.ForegroundInfo
import androidx.work.WorkerParameters
import dagger.assisted.Assisted
import dagger.assisted.AssistedInject
import kotlinx.coroutines.CancellationException
import java.util.logging.Logger

/**
 * Runs [CallWakeProcessor] for one call wake. One unique chain per channel
 * (`call-<channelId>`, replaced by every newer call wake). No retries: a
 * call is only relevant for a few seconds, and the name lookup already falls
 * back to a generic text.
 */
@HiltWorker
class CallWakeWorker @AssistedInject constructor(
    @Assisted ctx: Context,
    @Assisted params: WorkerParameters,
    private val processor: CallWakeProcessor,
) : CoroutineWorker(ctx, params) {

    companion object {
        private val logger = Logger.getLogger("CallWakeWorker")

        const val KEY_CALL_ID = "call_id"
        const val KEY_CALLER_ID = "caller_id"
        const val KEY_CALL_STATE = "call_state"
        const val KEY_CALL_REASON = "call_reason"

        fun uniqueWorkName(channelId: String) = "call-$channelId"

        fun inputData(signal: WakeSignal, owner: WakeOwner): Data {
            val builder = Data.Builder().putAll(WakeFetchWorker.inputData(signal, owner))
            signal.call?.let { call ->
                builder
                    .putString(KEY_CALL_ID, call.callId)
                    .putString(KEY_CALLER_ID, call.callerId)
                    .putString(KEY_CALL_STATE, call.state.name)
                    .putString(KEY_CALL_REASON, call.reason?.name)
            }
            return builder.build()
        }

        fun signalFrom(data: Data): WakeSignal? {
            val base = WakeFetchWorker.signalFrom(data) ?: return null
            val callId = data.getString(KEY_CALL_ID) ?: return null
            val callerId = data.getString(KEY_CALLER_ID) ?: return null
            val state = data.getString(KEY_CALL_STATE)
                ?.let { name -> CallState.entries.firstOrNull { it.name == name } }
                ?: CallState.UNKNOWN
            val reason = data.getString(KEY_CALL_REASON)?.let { name ->
                CallEndReason.entries.firstOrNull { it.name == name } ?: CallEndReason.UNKNOWN
            }
            return base.copy(call = CallInfo(callId, callerId, state, reason))
        }
    }

    override suspend fun doWork(): Result {
        val signal = signalFrom(inputData) ?: return Result.success()
        // A request without an account cannot be attributed: nothing to post.
        val owner = WakeFetchWorker.ownerFrom(inputData) ?: return Result.success()
        val outcome = try {
            processor.process(signal, owner) { isStopped }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("call wake failed (${e::class.simpleName})")
            return Result.success()
        }
        logger.info("call wake: state=${signal.call?.state} outcome=$outcome")
        return Result.success()
    }

    override suspend fun getForegroundInfo(): ForegroundInfo = wakeForegroundInfo(applicationContext)
}
