package io.wolftown.kaiku.data.keydevice

import io.wolftown.kaiku.approval.UvClass
import io.wolftown.kaiku.data.api.ApiErrorFields
import io.wolftown.kaiku.testutil.FakeApprovalApi
import io.wolftown.kaiku.testutil.SigFixtures
import io.wolftown.kaiku.testutil.apiError
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.time.Instant

/** The probe reports a key as gone only in the two answers the server's contract defines for it. */
class KeyDeviceProbeTest {

    private val api = FakeApprovalApi()
    /** The install id the app has now. */
    private var appInstallId = SigFixtures.DEVICE_ID
    private val probe = KeyDeviceProbe(api) { appInstallId }
    private val device = LocalKeyDevice("kd1", SigFixtures.USER_ID, SigFixtures.ORIGIN, UvClass.PER_USE, "e1", null)
    private val rec = StoredKeyDevice(device, KeyDevicePhase.ACTIVE, keyId = "k1", installId = SigFixtures.DEVICE_ID)

    private suspend fun answer(x: Exception): ServerKeyState {
        api.failWith = x
        return probe.probe(rec)
    }

    private suspend fun gone(x: Exception): ServerAnswer = (answer(x) as ServerKeyState.Gone).answer

    @Test fun `only 404 unknown and 403 revoked say the key is gone`() = runTest {
        assertEquals(ServerAnswer(404, "KEY_DEVICE_UNKNOWN"), gone(apiError(404, "KEY_DEVICE_UNKNOWN")))
        assertEquals(ServerAnswer(403, "KEY_DEVICE_REVOKED"), gone(apiError(403, "KEY_DEVICE_REVOKED")))
        assertEquals(ServerAnswer(404, "KEY_DEVICE_UNKNOWN"), gone(apiError(404, "key_device_unknown")))
    }

    @Test fun `the same codes with any other status keep the key`() = runTest {
        for (x in listOf(
            apiError(503, "KEY_DEVICE_UNKNOWN"),
            apiError(500, "KEY_DEVICE_REVOKED"),
            apiError(403, "KEY_DEVICE_UNKNOWN"),
            apiError(404, "KEY_DEVICE_REVOKED"),
            apiError(200, "KEY_DEVICE_UNKNOWN"),
            apiError(410, "KEY_DEVICE_REVOKED"),
            apiError(404, "NOT_FOUND"),
        )) {
            val st = answer(x)
            assertTrue(x.toString(), st == ServerKeyState.Unknown || st == ServerKeyState.Refused)
        }
    }

    @Test fun `pending and offline are told apart`() = runTest {
        val at = Instant.parse("2026-11-20T08:00:00Z")
        assertEquals(ServerKeyState.Pending(at), answer(apiError(409, "KEY_DEVICE_PENDING", ApiErrorFields(readyAt = at.toString()))))
        assertEquals(ServerKeyState.Unknown, answer(java.io.IOException("offline")))
    }

    /**
     * "The server could not be asked" is no connection, a timeout, a server fault, "too many
     * requests" or a page that is not the server's. A refusal the server itself gave is an answer:
     * it keeps the key just the same, but the server is there.
     */
    @Test fun `no answer and a refusal of the server itself are told apart`() = runTest {
        for (x in listOf<Exception>(
            java.io.IOException("offline"), java.net.SocketTimeoutException("timeout"),
            apiError(500, "INTERNAL_ERROR"), apiError(502, "KEY_DEVICE_UNKNOWN"), apiError(503, "SERVICE_UNAVAILABLE"),
            apiError(429, "TOO_MANY_ATTEMPTS"),
            // A captive portal's or a proxy's page: a status without the server's code.
            io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.Forbidden, "blocked"),
            io.wolftown.kaiku.data.api.ApiException(io.ktor.http.HttpStatusCode.NotFound, "not found"),
            kotlinx.serialization.SerializationException("not json"),
        )) {
            assertTrue(x.toString(), x.isNoAnswer())
            assertEquals(x.toString(), ServerKeyState.Unknown, answer(x))
        }
        for (x in listOf(
            apiError(403, "ACCOUNT_BANNED"), apiError(400, "VALIDATION_ERROR"), apiError(410, "APPROVAL_EXPIRED"),
            apiError(404, "NOT_FOUND"), apiError(409, "KEY_DEVICE_PENDING"),
        )) {
            assertFalse(x.toString(), x.isNoAnswer())
            assertEquals(x.toString(), ServerKeyState.Refused, answer(x))
        }
    }

    /**
     * The server lists a key device by account and install id. The question names the id the
     * record was enrolled under: an id the app got later (its own file was lost and made anew)
     * would be answered "no such key device" for a key the server still lists.
     */
    @Test fun `the question names the install id of the record, whatever id the app has now`() = runTest {
        appInstallId = "0a0a0a0a-0a0a-4a0a-8a0a-0a0a0a0a0a0a"
        answer(apiError(404, "KEY_DEVICE_UNKNOWN"))
        assertEquals(listOf("deviceLoginChallenge:${SigFixtures.USER_ID}:${SigFixtures.DEVICE_ID}"), api.calls)
        assertEquals(SigFixtures.ORIGIN, api.deviceLoginOrigin)
    }

    @Test fun `a record from before it named its install id is asked about with the app's id`() = runTest {
        api.failWith = apiError(404, "KEY_DEVICE_UNKNOWN")
        probe.probe(rec.copy(installId = null))
        assertEquals(listOf("deviceLoginChallenge:${SigFixtures.USER_ID}:${SigFixtures.DEVICE_ID}"), api.calls)
    }

    @Test fun `every 'no key device' of the probe says when its question was sent`() = runTest {
        val before = AskOrder.next()
        val first = (answer(apiError(404, "KEY_DEVICE_UNKNOWN")) as ServerKeyState.Gone).sent!!
        val between = AskOrder.next()
        val second = (answer(apiError(403, "KEY_DEVICE_REVOKED")) as ServerKeyState.Gone).sent!!
        assertTrue(first.isAfter(before) && !first.isAfter(between))
        assertTrue(second.isAfter(between))
        // One that the probe did not make says nothing about it.
        assertEquals(null, ServerKeyState.Gone(ServerAnswer(404, "KEY_DEVICE_UNKNOWN")).sent)
    }
}
