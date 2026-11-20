package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.DeviceIdProvider
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.api.ApprovalApi
import java.time.Instant
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/**
 * When a probe's question was sent, in the order of [AskOrder]. Only [KeyDeviceProbe] makes one
 * (its one implementation is private to that class), so an answer can be required to belong to a
 * question the probe really sent after some other answer was taken.
 */
sealed interface ProbeSent {
    fun isAfter(order: Long): Boolean
}

/** What the server says about this phone's key device for a user (see [KeyDeviceProbe]). */
sealed interface ServerKeyState {
    /** An active key device: device-key sign-in works. */
    data object Active : ServerKeyState
    data class Pending(val readyAt: Instant) : ServerKeyState
    /**
     * No key device for this account on this phone, in one of the server's two answers for that
     * ([answer]). What it means for a stored record, [KeyAnswers.settle] decides. [sent]: when
     * the question was sent; only the probe sets it.
     */
    data class Gone(val answer: ServerAnswer, val sent: ProbeSent? = null) : ServerKeyState
    /**
     * The server itself answered, but not about the key device (the account is suspended, the
     * request was refused): nothing may be deleted, and the server is there.
     */
    data object Refused : ServerKeyState
    /** No answer of the server itself (see [isNoAnswer]): nothing may be deleted. */
    data object Unknown : ServerKeyState
}

/**
 * Whether a failed request brought nothing the server itself said: no connection, a timeout, or a
 * body that is not the server's (no error code of its own). Any answer with the server's own error
 * code, also "too many requests" or a fault it reports, is an answer.
 */
fun Exception.isNoAnswer(): Boolean = when (this) {
    is ApiException -> normalizedCode == null
    else -> true
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
    private class Sent(private val order: Long) : ProbeSent {
        override fun isAfter(order: Long): Boolean = this.order > order
        override fun toString() = "Sent"
    }

    /**
     * Asks about the key device of [rec]: its account, on its server, under the install id it was
     * enrolled with ([StoredKeyDevice.installId]; the app's current id only for a record that does
     * not name one). An id the app got later never stands in for it, so "no such key device" is
     * always an answer about this record's own key device.
     */
    suspend fun probe(rec: StoredKeyDevice): ServerKeyState {
        // Taken before the request leaves: the answer belongs to a question sent at this point.
        val sent = Sent(AskOrder.next())
        return ask(rec.device.userId, rec.installId ?: deviceIds.id(), rec.device.origin, sent)
    }

    private suspend fun ask(userId: String, installId: String, origin: String, sent: ProbeSent): ServerKeyState = try {
        approvalApi.deviceLoginChallenge(userId, installId, origin)
        ServerKeyState.Active
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        when {
            e is ApiException && e.saysKeyDeviceGone() -> ServerKeyState.Gone(ServerAnswer.of(e), sent)
            e is ApiException && e.normalizedCode == "KEY_DEVICE_PENDING" ->
                e.fields.readyAt?.let { runCatching { Instant.parse(it) }.getOrNull() }?.let { ServerKeyState.Pending(it) }
                    ?: if (e.isNoAnswer()) ServerKeyState.Unknown else ServerKeyState.Refused
            // Offline, a server fault, an unreadable answer (e.g. a captive portal's page): not known.
            e.isNoAnswer() -> ServerKeyState.Unknown
            else -> ServerKeyState.Refused
        }
    }
}
