package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.repository.AuthRepository
import java.util.logging.Logger
import javax.inject.Inject
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull

/**
 * B.1: Android invalidated K_approve (the screen lock was removed). Wherever a
 * signature found out, the device is reported while a session still works (the
 * unsigned `invalidated` route; the server revokes it and notifies), and the key
 * and its metadata are forgotten. Never replaced silently: the phone is set up again.
 */
class KeyInvalidation @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val keyDevices: KeyDeviceRepository,
    private val authRepository: AuthRepository,
) {
    private companion object {
        val logger: Logger = Logger.getLogger("KeyInvalidation")
        const val REPORT_TIMEOUT_MS = 5_000L
    }

    /**
     * Runs to the end even when the screen that found out is left meanwhile: the report is
     * bounded, and the key and its record are always forgotten.
     */
    suspend fun handle() = withContext(NonCancellable) {
        try {
            val id = keyDevices.local()?.keyDeviceId
            if (id != null && authRepository.hasLiveSession()) {
                try {
                    withTimeoutOrNull(REPORT_TIMEOUT_MS) { approvalApi.reportInvalidated(id) }
                } catch (e: Exception) {
                    logger.warning("Invalidated key not reported: ${e.javaClass.simpleName}")
                }
            }
        } finally {
            keyDevices.forget()
        }
    }
}
