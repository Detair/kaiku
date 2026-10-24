package io.wolftown.kaiku.push

import android.content.Context
import androidx.core.app.NotificationCompat
import androidx.hilt.work.HiltWorker
import androidx.work.CoroutineWorker
import androidx.work.Data
import androidx.work.ForegroundInfo
import androidx.work.WorkerParameters
import dagger.assisted.Assisted
import dagger.assisted.AssistedInject
import io.wolftown.kaiku.R
import kotlinx.coroutines.CancellationException
import java.util.logging.Logger

/**
 * Runs [WakeFetchProcessor] for one wake signal. Retries a transient failure
 * at most [MAX_ATTEMPTS] times in total, then gives up quietly: the catch-up
 * on the next app start covers anything missed.
 */
@HiltWorker
class WakeFetchWorker @AssistedInject constructor(
    @Assisted ctx: Context,
    @Assisted params: WorkerParameters,
    private val processor: WakeFetchProcessor,
) : CoroutineWorker(ctx, params) {

    companion object {
        private val logger = Logger.getLogger("WakeFetchWorker")

        const val KEY_CHANNEL_ID = "channel_id"
        const val KEY_KIND = "kind"
        const val KEY_COUNT = "count"
        const val KEY_USER_ID = "user_id"
        const val KEY_SERVER_URL = "server_url"

        /** Tag of every wake fetch request (cancelled together on logout). */
        const val WORK_TAG = "wake"
        const val MAX_ATTEMPTS = 3

        /** Id of the silent "checking for new messages" notification (expedited work before API 31). */
        private const val FOREGROUND_NOTIFICATION_ID = 0x4B41

        fun uniqueWorkName(channelId: String) = "wake-$channelId"

        fun inputData(signal: WakeSignal, owner: WakeOwner): Data = Data.Builder()
            .putString(KEY_CHANNEL_ID, signal.channelId)
            .putString(KEY_KIND, signal.kind.name)
            .putInt(KEY_COUNT, signal.count)
            .putString(KEY_USER_ID, owner.userId)
            .putString(KEY_SERVER_URL, owner.serverUrl)
            .build()

        fun ownerFrom(data: Data): WakeOwner? {
            val userId = data.getString(KEY_USER_ID) ?: return null
            return WakeOwner(userId, data.getString(KEY_SERVER_URL))
        }

        fun signalFrom(data: Data): WakeSignal? {
            val channelId = data.getString(KEY_CHANNEL_ID) ?: return null
            val kind = data.getString(KEY_KIND)
                ?.let { name -> WakeKind.entries.firstOrNull { it.name == name } }
                ?: WakeKind.UNKNOWN
            return WakeSignal(kind, channelId, data.getInt(KEY_COUNT, 1))
        }
    }

    override suspend fun doWork(): Result {
        val signal = signalFrom(inputData) ?: return Result.success()
        // A request without an account cannot be attributed: nothing to post.
        val owner = ownerFrom(inputData) ?: return Result.success()
        val outcome = try {
            processor.process(signal, owner)
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            WakeOutcome.RETRY
        }
        logger.info("wake fetch: kind=${signal.kind} outcome=$outcome attempt=${runAttemptCount + 1}")
        return if (outcome == WakeOutcome.RETRY && runAttemptCount < MAX_ATTEMPTS - 1) {
            Result.retry()
        } else {
            Result.success()
        }
    }

    override suspend fun getForegroundInfo(): ForegroundInfo {
        val notification = NotificationCompat.Builder(applicationContext, NotificationChannels.SYNC)
            .setSmallIcon(android.R.drawable.stat_notify_sync)
            .setContentTitle(applicationContext.getString(R.string.notif_checking_messages))
            .setPriority(NotificationCompat.PRIORITY_MIN)
            .setSilent(true)
            .setOngoing(true)
            .build()
        return ForegroundInfo(FOREGROUND_NOTIFICATION_ID, notification)
    }
}
