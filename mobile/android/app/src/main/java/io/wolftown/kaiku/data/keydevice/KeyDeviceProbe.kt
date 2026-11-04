package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import java.io.IOException
import java.time.Instant
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/** What the server says about this phone's key device for a user (see [KeyDeviceProbe]). */
sealed interface ServerKeyState {
    /** An active key device: device-key sign-in works. */
    data object Active : ServerKeyState
    data class Pending(val readyAt: Instant) : ServerKeyState
    /** No key device (the claim never landed, was not confirmed yet, or the setup was undone). */
    data object Gone : ServerKeyState
    /** Not known now (offline, server trouble): nothing may be deleted. */
    data object Unknown : ServerKeyState
}

/**
 * Asks the server where this phone's key device stands without a claim secret
 * (a self-claim never gets one, a lost claim answer never delivered one): the
 * public `POST /auth/device-login/challenge` (C26) answers with a challenge for
 * an active key device, 409 `KEY_DEVICE_PENDING {ready_at}` for a pending one,
 * and 404 `KEY_DEVICE_UNKNOWN` / 403 `KEY_DEVICE_REVOKED` otherwise. The
 * challenge is not answered; it expires unused. It always asks the key's own
 * server ([origin]): another server's "unknown" must never cost a key (Fable m1).
 */
class KeyDeviceProbe @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val deviceIds: DeviceIdProvider,
) {
    suspend fun probe(userId: String, origin: String): ServerKeyState = try {
        approvalApi.deviceLoginChallenge(userId, deviceIds.id(), origin)
        ServerKeyState.Active
    } catch (e: CancellationException) {
        throw e
    } catch (e: ApiException) {
        when (e.normalizedCode) {
            "KEY_DEVICE_PENDING" -> e.fields.readyAt?.let { runCatching { Instant.parse(it) }.getOrNull() }
                ?.let { ServerKeyState.Pending(it) } ?: ServerKeyState.Unknown
            "KEY_DEVICE_UNKNOWN", "KEY_DEVICE_REVOKED" -> ServerKeyState.Gone
            else -> ServerKeyState.Unknown
        }
    } catch (_: IOException) {
        ServerKeyState.Unknown
    } catch (_: Exception) {
        // An unreadable answer (e.g. a captive portal's page): not known, nothing is deleted.
        ServerKeyState.Unknown
    }
}
