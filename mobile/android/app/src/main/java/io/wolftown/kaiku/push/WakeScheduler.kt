package io.wolftown.kaiku.push

import android.content.Context
import androidx.work.Constraints
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.OutOfQuotaPolicy
import androidx.work.WorkManager
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.data.local.TokenStorage
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/** The account a wake fetch belongs to (no tokens). */
data class WakeOwner(val userId: String, val serverUrl: String?)

/** Schedules and cancels the background fetches for wake signals. */
interface WakeWorkEnqueuer {
    /** Schedules a fetch for [signal] on behalf of the signed-in account; no-op when signed out. */
    fun enqueue(signal: WakeSignal)

    /**
     * Schedules the quick call path for a call wake (no message fetch); a
     * newer call wake of the same channel replaces it. No-op when signed out.
     */
    fun enqueueCall(signal: WakeSignal)

    /** Drops queued or running fetches of [channelId] (the channel was read); call work stays. */
    fun cancel(channelId: String)

    /** Drops every queued or running fetch and call work (logout, account change). */
    fun cancelAll()
}

/**
 * [WakeWorkEnqueuer] on WorkManager: expedited when quota allows, needs a
 * network, one chain per channel (`wake-<channelId>`) so wakes for the same
 * channel run one after another. Every request carries its account
 * ([WakeOwner]) and the tag [WakeFetchWorker.WORK_TAG]. Call wakes run in
 * [CallWakeWorker] (`call-<channelId>`, REPLACE, no network constraint: the
 * name lookup has an offline fallback).
 */
class WorkManagerWakeEnqueuer @Inject constructor(
    @ApplicationContext private val context: Context,
    private val tokenStorage: TokenStorage,
) : WakeWorkEnqueuer {

    private val workManager: WorkManager
        get() = WorkManager.getInstance(context)

    override fun enqueue(signal: WakeSignal) {
        val userId = tokenStorage.getUserId() ?: return
        val owner = WakeOwner(userId, tokenStorage.getServerUrl())
        val request = OneTimeWorkRequestBuilder<WakeFetchWorker>()
            .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            .setConstraints(
                Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
            )
            .setInputData(WakeFetchWorker.inputData(signal, owner))
            .addTag(WakeFetchWorker.WORK_TAG)
            .build()
        workManager.enqueueUniqueWork(
            WakeFetchWorker.uniqueWorkName(signal.channelId),
            ExistingWorkPolicy.APPEND_OR_REPLACE,
            request,
        )
    }

    override fun enqueueCall(signal: WakeSignal) {
        val userId = tokenStorage.getUserId() ?: return
        val owner = WakeOwner(userId, tokenStorage.getServerUrl())
        val request = OneTimeWorkRequestBuilder<CallWakeWorker>()
            .setExpedited(OutOfQuotaPolicy.RUN_AS_NON_EXPEDITED_WORK_REQUEST)
            .setInputData(CallWakeWorker.inputData(signal, owner))
            .addTag(WakeFetchWorker.WORK_TAG)
            .build()
        workManager.enqueueUniqueWork(
            CallWakeWorker.uniqueWorkName(signal.channelId),
            ExistingWorkPolicy.REPLACE,
            request,
        )
    }

    override fun cancel(channelId: String) {
        workManager.cancelUniqueWork(WakeFetchWorker.uniqueWorkName(channelId))
    }

    override fun cancelAll() {
        workManager.cancelAllWorkByTag(WakeFetchWorker.WORK_TAG)
    }
}

/** Entry point for both push transports: routes a parsed wake signal. */
@Singleton
class WakeScheduler @Inject constructor(
    private val enqueuer: WakeWorkEnqueuer,
    private val sink: NotificationSink,
    private val callIntake: CallWakeIntake,
) {
    companion object {
        private val logger = Logger.getLogger("WakeScheduler")
    }

    fun onWake(signal: WakeSignal) {
        logger.info("wake received: kind=${signal.kind}")
        when (signal.kind) {
            WakeKind.DM, WakeKind.MENTION -> enqueuer.enqueue(signal)
            // Read on another device: clear the notification and stop pending
            // fetches. The push carries no position; a late wake for the same
            // messages is held back by the server's unread count (C20).
            WakeKind.READ -> {
                enqueuer.cancel(signal.channelId)
                sink.cancel(signal.channelId)
            }
            // DM call: the quick call path (name lookup + notification), never a fetch.
            // Only wakes that still apply are queued: each one replaces the channel's pending call work.
            WakeKind.CALL -> if (callIntake.accept(signal)) enqueuer.enqueueCall(signal)
            WakeKind.UNKNOWN -> Unit
        }
    }
}
