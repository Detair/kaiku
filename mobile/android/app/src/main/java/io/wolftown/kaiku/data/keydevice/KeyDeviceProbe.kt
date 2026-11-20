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
    /**
     * No key device for this account on this phone, in one of the server's two answers for that
     * ([answer]). What it means for a stored record, [KeyAnswers.settle] decides. [askedAt]: when
     * the question was sent ([AskOrder]).
     */
    data class Gone(val answer: ServerAnswer, val askedAt: Long = 0) : ServerKeyState
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
 * server ([origin]): another server's "unknown" must never cost a key. Only those
 * two exact answers mean [ServerKeyState.Gone] ([saysKeyDeviceGone]).
 */
class KeyDeviceProbe @Inject constructor(
    private val approvalApi: ApprovalApi,
    private val deviceIds: DeviceIdProvider,
) {
    /**
     * Asks about the key device of [rec]: its account, on its server, under the install id it was
     * enrolled with ([StoredKeyDevice.installId]; the app's current id only for a record that does
     * not name one). An id the app got later never stands in for it, so "no such key device" is
     * always an answer about this record's own key device.
     */
    suspend fun probe(rec: StoredKeyDevice): ServerKeyState {
        val asked = AskOrder.next()
        return ask(rec.device.userId, rec.installId ?: deviceIds.id(), rec.device.origin, asked)
    }

    private suspend fun ask(userId: String, installId: String, origin: String, asked: Long): ServerKeyState = try {
        approvalApi.deviceLoginChallenge(userId, installId, origin)
        ServerKeyState.Active
    } catch (e: CancellationException) {
        throw e
    } catch (e: ApiException) {
        when {
            e.saysKeyDeviceGone() -> ServerKeyState.Gone(ServerAnswer.of(e), asked)
            e.normalizedCode == "KEY_DEVICE_PENDING" -> e.fields.readyAt?.let { runCatching { Instant.parse(it) }.getOrNull() }
                ?.let { ServerKeyState.Pending(it) } ?: ServerKeyState.Unknown
            else -> ServerKeyState.Unknown
        }
    } catch (_: IOException) {
        ServerKeyState.Unknown
    } catch (_: Exception) {
        // An unreadable answer (e.g. a captive portal's page): not known, nothing is deleted.
        ServerKeyState.Unknown
    }
}
