package io.wolftown.kaiku.ui.dm

import io.ktor.http.HttpStatusCode
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.PeopleUnavailableException
import org.junit.Assert.assertEquals
import org.junit.Test
import java.io.IOException

class DmErrorTextTest {
    private fun api(status: HttpStatusCode, code: String? = null) = ApiException(status, "server text", code = code)

    @Test
    fun `blocked, unknown and forbidden all read the same neutral sentence`() {
        assertEquals(R.string.dm_new_not_possible, DmErrorText.open(api(HttpStatusCode.BadRequest, "VALIDATION_ERROR")))
        assertEquals(R.string.dm_new_not_possible, DmErrorText.open(api(HttpStatusCode.BadRequest, "validation_error")))
        assertEquals(R.string.dm_new_not_possible, DmErrorText.open(api(HttpStatusCode.Forbidden)))
        assertEquals(R.string.dm_new_not_possible, DmErrorText.open(api(HttpStatusCode.NotFound)))
    }

    @Test
    fun `network, server, rate limit and anything else`() {
        assertEquals(R.string.errors_network, DmErrorText.open(IOException()))
        assertEquals(R.string.errors_server, DmErrorText.open(api(HttpStatusCode.BadGateway)))
        assertEquals(R.string.errors_rate_limited, DmErrorText.open(api(HttpStatusCode.TooManyRequests)))
        assertEquals(R.string.dm_new_create_failed, DmErrorText.open(IllegalStateException()))
    }

    @Test
    fun `loading people uses the cause of the failure`() {
        assertEquals(R.string.errors_network, DmErrorText.load(PeopleUnavailableException(IOException())))
        assertEquals(R.string.dm_new_load_failed, DmErrorText.load(PeopleUnavailableException(null)))
        assertEquals(R.string.errors_server, DmErrorText.load(api(HttpStatusCode.InternalServerError)))
    }
}
