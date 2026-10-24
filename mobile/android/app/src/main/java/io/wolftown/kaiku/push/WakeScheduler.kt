package io.wolftown.kaiku.push

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkManager
import dagger.hilt.android.qualifiers.ApplicationContext
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/** Schedules the background fetch for a wake signal. */
fun interface WakeWorkEnqueuer {
    fun enqueue(signal: WakeSignal)
}

/**
 * [WakeWorkEnqueuer] on WorkManager: expedited when quota allows, needs a
 * network, one chain per channel (`wake-<channelId>`) so wakes for the same
 * channel run one after another.
 */
class WorkManagerWakeEnqueuer @Inject constructor(
    @ApplicationContext private val context: Context,
) : WakeWorkEnqueuer {
    override fun enqueue(signal: WakeSignal) {
        val request = OneTimeWorkRequestBuilder<WakeFetchWorker>()
            .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            .setConstraints(
                Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
            )
            .setInputData(WakeFetchWorker.inputData(signal))
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(
            WakeFetchWorker.uniqueWorkName(signal.channelId),
            ExistingWorkPolicy.APPEND_OR_REPLACE,
            request,
        )
    }
}

/** Entry point for both push transports: routes a parsed wake signal. */
@Singleton
class WakeScheduler @Inject constructor(
    private val enqueuer: WakeWorkEnqueuer,
    private val sink: NotificationSink,
) {
    companion object {
        private val logger = Logger.getLogger("WakeScheduler")
    }

    fun onWake(signal: WakeSignal) {
        logger.info("wake received: kind=${signal.kind}")
        when (signal.kind) {
            WakeKind.DM, WakeKind.MENTION -> enqueuer.enqueue(signal)
            // Read on another device: clear the notification, nothing to fetch.
            WakeKind.READ -> sink.cancel(signal.channelId)
            WakeKind.CALL, WakeKind.UNKNOWN -> Unit
        }
    }
}
