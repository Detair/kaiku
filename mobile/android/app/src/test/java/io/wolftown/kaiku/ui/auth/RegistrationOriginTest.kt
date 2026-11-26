package io.wolftown.kaiku.ui.auth

import io.ktor.client.engine.mock.MockEngine
import io.ktor.client.engine.mock.respond
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.headersOf
import io.mockk.every
import io.mockk.mockk
import io.wolftown.kaiku.data.api.AuthApiImpl
import io.wolftown.kaiku.data.api.EnrollmentApiImpl
import io.wolftown.kaiku.data.api.KaikuHttpClient
import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.data.repository.AuthRepository
import io.wolftown.kaiku.push.PushRegistrar
import io.wolftown.kaiku.testutil.noTeardown
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/**
 * A registration is made on the server it started with: its ticket is never sent anywhere else,
 * also when the app's server address changes while the account is being created. Requests are
 * counted per host on the (mocked) wire, through the app's own HTTP client.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class RegistrationOriginTest {

    private val a = "https://chat.example.com"
    private val b = "https://other.example.org"
    private var configured: String? = a
    private val hosts = mutableListOf<String>()
    private val ticketHosts = mutableListOf<String>()

    @Before fun setUp() = Dispatchers.setMain(StandardTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    @Test fun `a ticket goes only to the server the registration started with`() = runTest {
        val tokens: TokenStorage = mockk(relaxed = true) {
            every { getServerUrl() } answers { configured }
            every { getAccessToken() } returns null
        }
        val authState = AuthState(CoroutineScope(SupervisorJob() + UnconfinedTestDispatcher(testScheduler)))
        val client = KaikuHttpClient.forTesting(tokens, authState, MockEngine { request ->
            val url = request.url.toString()
            hosts += request.url.host
            if (request.headers["X-Kaiku-Ticket"] != null) ticketHosts += request.url.host
            val json = headersOf(HttpHeaders.ContentType, "application/json")
            when {
                url.endsWith("/auth/register") -> {
                    // While the account is created, the app's server address changes.
                    configured = b
                    respond(
                        """{"user_id":"u1","username":"anna","ticket_expires_at":"2026-11-20T12:30:00Z","registration_ticket":"ticket-secret"}""",
                        HttpStatusCode.Created, json,
                    )
                }
                url.endsWith("/auth/registration/enrollments") -> respond(
                    """{"enrollment_id":"e1","setup_token":"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8","setup_url":"$a/setup#v=1&t=x","expires_at":"2026-11-20T12:10:00Z"}""",
                    HttpStatusCode.Created, json,
                )
                else -> respond("{}", HttpStatusCode.NotFound, json)
            }
        }).activeClient()
        val repo = AuthRepository(AuthApiImpl(client), tokens, authState, mockk<PushRegistrar>(relaxed = true), noTeardown(authState))
        val registrations = Registrations()
        val vm = RegisterViewModel(repo, EnrollmentApiImpl(client), tokens, registrations)

        vm.onUsernameChanged("anna")
        vm.onPasswordChanged("a long enough password")
        vm.onRegisterClicked()
        advanceUntilIdle()
        // The requests run on the client's own threads: wait for the registration to finish.
        vm.uiState.first { !it.isLoading }

        assertEquals(listOf("chat.example.com", "chat.example.com"), hosts)
        assertEquals(listOf("chat.example.com"), ticketHosts)
        assertEquals(a, vm.uiState.value.setupEntry?.origin)
        assertTrue(!registrations.running())
    }
}
