package io.wolftown.kaiku.push

import android.content.Context
import androidx.hilt.work.HiltWorker
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import dagger.assisted.Assisted
import dagger.assisted.AssistedInject
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.coroutines.CancellationException
import java.util.concurrent.TimeUnit
import java.util.logging.Logger
import javax.inject.Inject

/** Schedules the background registration retry. */
interface PushRegistrationScheduler {
    /** (Re)schedules one registration attempt once a network is available. */
    fun schedule()

    /** Drops a scheduled or running attempt (logout). */
    fun cancel()
}

class WorkManagerPushRegistrationScheduler @Inject constructor(
    @ApplicationContext private val context: Context,
) : PushRegistrationScheduler {

    override fun schedule() {
        val request = OneTimeWorkRequestBuilder<PushRegistrationWorker>()
            .setConstraints(
                Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
            )
            .setBackoffCriteria(
                BackoffPolicy.EXPONENTIAL,
                PushRegistrationWorker.BACKOFF_SECONDS,
                TimeUnit.SECONDS,
            )
            .build()
        WorkManager.getInstance(context).enqueueUniqueWork(
            PushRegistrationWorker.UNIQUE_NAME,
            ExistingWorkPolicy.REPLACE,
            request,
        )
    }

    override fun cancel() {
        WorkManager.getInstance(context).cancelUniqueWork(PushRegistrationWorker.UNIQUE_NAME)
    }
}

/**
 * Retries the push registration after a network failure, with exponential
 * backoff from [BACKOFF_SECONDS]. Gives up after [MAX_ATTEMPTS]; the next app
 * start or token refresh registers again.
 */
@HiltWorker
class PushRegistrationWorker @AssistedInject constructor(
    @Assisted ctx: Context,
    @Assisted params: WorkerParameters,
    private val registrar: PushRegistrar,
) : CoroutineWorker(ctx, params) {

    companion object {
        private val logger = Logger.getLogger("PushRegistrationWorker")

        const val UNIQUE_NAME = "push-register"
        const val BACKOFF_SECONDS = 30L
        const val MAX_ATTEMPTS = 8
    }

    override suspend fun doWork(): Result {
        val outcome = try {
            // Never schedules itself again: a REPLACE would cancel this very run.
            registrar.refresh(scheduleRetry = false)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            logger.info("push registration retry failed: ${e.javaClass.simpleName}")
            RefreshOutcome.RETRY
        }
        logger.info("push registration retry: outcome=$outcome attempt=${runAttemptCount + 1}")
        return if (outcome == RefreshOutcome.RETRY && runAttemptCount < MAX_ATTEMPTS - 1) {
            Result.retry()
        } else {
            Result.success()
        }
    }
}
