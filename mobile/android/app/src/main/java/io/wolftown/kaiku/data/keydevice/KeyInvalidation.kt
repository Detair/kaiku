package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.data.api.ApprovalApi
import io.wolftown.kaiku.data.repository.AuthRepository
import java.util.logging.Logger
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

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
    }

    suspend fun handle() {
        val id = keyDevices.local()?.keyDeviceId
        if (id != null && authRepository.hasLiveSession()) {
            try {
                approvalApi.reportInvalidated(id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.warning("Invalidated key not reported: ${e.javaClass.simpleName}")
            }
        }
        keyDevices.forget()
    }
}
