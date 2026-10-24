package io.wolftown.kaiku.data.ws

import app.cash.turbine.test
import kotlinx.coroutines.test.runTest
import okhttp3.OkHttpClient
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import kotlin.test.assertIs
import kotlin.time.Duration.Companion.seconds

class KaikuWebSocketTest {

    private lateinit var mockServer: MockWebServer
    private lateinit var okHttpClient: OkHttpClient
    private lateinit var ws: KaikuWebSocket

    @Before
    fun setUp() {
        mockServer = MockWebServer()
        mockServer.start()

        okHttpClient = OkHttpClient.Builder().build()

        ws = KaikuWebSocket(okHttpClient, WsJson)
    }

    private fun serverUrl() = mockServer.url("/").toString()

    @After
    fun tearDown() {
        ws.disconnect()
        try {
            mockServer.shutdown()
        } catch (_: Exception) {
            // MockWebServer may timeout waiting for the queue to drain after
            // WebSocket close handshake; this is safe to ignore in tests.
        }
    }

    @Test
    fun `initial state is Disconnected`() {
        assertEquals(ConnectionState.Disconnected, ws.connectionState.value)
    }

    @Test
    fun `connect transitions to Connecting then Connected`() = runTest {
        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    webSocket.send("""{"type":"ready","user_id":"usr-001"}""")
                }
            })
        )

        ws.connectionState.test(timeout = 10.seconds) {
            assertEquals(ConnectionState.Disconnected, awaitItem())

            ws.connect(serverUrl(), TOKEN)

            assertEquals(ConnectionState.Connecting, awaitItem())
            assertEquals(ConnectionState.Connected, awaitItem())

            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `connect stays in Connecting state until Ready event arrives`() = runTest {
        val authLatch = CountDownLatch(1)

        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    // Don't send anything — wait for Authenticate frame
                }

                override fun onMessage(webSocket: WebSocket, text: String) {
                    // Receive Authenticate but don't respond with Ready
                    authLatch.countDown()
                }
            })
        )

        ws.connectionState.test(timeout = 10.seconds) {
            assertEquals(ConnectionState.Disconnected, awaitItem())

            ws.connect(serverUrl(), TOKEN)
            assertEquals(ConnectionState.Connecting, awaitItem())

            // Wait for the auth frame to be received by mock server
            assertTrue(
                "Server did not receive auth message in time",
                authLatch.await(5, TimeUnit.SECONDS)
            )

            // Give the WebSocket message loop time to process anything the server
            // would have sent. Without this real delay, expectNoEvents() returns
            // immediately and would pass even if the production code transitioned
            // to Connected directly from onOpen.
            kotlinx.coroutines.delay(200)

            // No transition to Connected — server hasn't sent Ready
            expectNoEvents()

            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `events flow emits parsed server events`() = runTest {
        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    webSocket.send("""{"type":"ready","user_id":"usr-001"}""")
                    webSocket.send("""{"type":"message_new","channel_id":"chan-001","message":{"id":"msg-1","content":"hello"}}""")
                    webSocket.send("""{"type":"typing_start","channel_id":"chan-001","user_id":"usr-002"}""")
                }
            })
        )

        ws.events.test(timeout = 10.seconds) {
            ws.connect(serverUrl(), TOKEN)

            val ready = awaitItem()
            assertIs<ServerEvent.Ready>(ready)
            assertEquals("usr-001", ready.userId)

            val messageNew = awaitItem()
            assertIs<ServerEvent.MessageNew>(messageNew)
            assertEquals("chan-001", messageNew.channelId)

            val typing = awaitItem()
            assertIs<ServerEvent.TypingStart>(typing)
            assertEquals("chan-001", typing.channelId)
            assertEquals("usr-002", typing.userId)

            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `send serializes ClientEvent and sends over WebSocket`() = runTest {
        val receivedMessages = mutableListOf<String>()
        // Count 2: one for the automatic Authenticate frame, one for Subscribe
        val messageLatch = CountDownLatch(2)

        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    // Server sends ready after receiving the authenticate frame
                }

                override fun onMessage(webSocket: WebSocket, text: String) {
                    receivedMessages.add(text)
                    messageLatch.countDown()
                    // Send ready after authenticate so the client can proceed
                    if (text.contains("authenticate")) {
                        webSocket.send("""{"type":"ready","user_id":"usr-001"}""")
                    }
                }
            })
        )

        ws.events.test(timeout = 10.seconds) {
            ws.connect(serverUrl(), TOKEN)
            awaitItem() // Ready

            ws.send(ClientEvent.Subscribe("chan-001"))

            // Wait for both messages to arrive at the mock server (max 5s)
            assertTrue("Server did not receive messages in time", messageLatch.await(5, TimeUnit.SECONDS))

            val sent = receivedMessages.firstOrNull { it.contains("subscribe") }
            assertTrue("Expected subscribe message to be sent", sent != null)
            assertTrue("Expected channel_id field", sent!!.contains(""""channel_id":"chan-001""""))
            assertTrue("Expected type field", sent.contains(""""type":"subscribe""""))

            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `disconnect transitions to Disconnected`() = runTest {
        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    webSocket.send("""{"type":"ready","user_id":"usr-001"}""")
                }
            })
        )

        ws.connectionState.test(timeout = 10.seconds) {
            assertEquals(ConnectionState.Disconnected, awaitItem())

            ws.connect(serverUrl(), TOKEN)
            assertEquals(ConnectionState.Connecting, awaitItem())
            assertEquals(ConnectionState.Connected, awaitItem())

            ws.disconnect()
            assertEquals(ConnectionState.Disconnected, awaitItem())

            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `connect sends Authenticate frame instead of header`() = runTest {
        val receivedMessages = mutableListOf<String>()
        val authLatch = CountDownLatch(1)

        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    // Don't send ready until we receive the auth frame
                }

                override fun onMessage(webSocket: WebSocket, text: String) {
                    receivedMessages.add(text)
                    authLatch.countDown()
                    // After receiving auth, send ready
                    webSocket.send("""{"type":"ready","user_id":"usr-001"}""")
                }
            })
        )

        ws.events.test(timeout = 10.seconds) {
            ws.connect(serverUrl(), TOKEN)

            // Wait for the authenticate message to arrive at the mock server
            assertTrue("Server did not receive auth message in time", authLatch.await(5, TimeUnit.SECONDS))

            // Verify no Sec-WebSocket-Protocol header was sent
            val request = mockServer.takeRequest()
            val protocol = request.getHeader("Sec-WebSocket-Protocol")
            assertEquals(null, protocol)

            // Verify the first message is the authenticate frame
            val authMsg = receivedMessages.first()
            assertTrue("Expected type field", authMsg.contains(""""type":"authenticate""""))
            assertTrue("Expected token field", authMsg.contains(""""token":"test-jwt-token""""))

            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `server close triggers Disconnected state`() = runTest {
        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    webSocket.send("""{"type":"ready","user_id":"usr-001"}""")
                    webSocket.close(1000, "Server shutdown")
                }
            })
        )

        ws.connectionState.test(timeout = 10.seconds) {
            assertEquals(ConnectionState.Disconnected, awaitItem())

            ws.connect(serverUrl(), TOKEN)
            assertEquals(ConnectionState.Connecting, awaitItem())
            assertEquals(ConnectionState.Connected, awaitItem())

            // Server-initiated close should transition back to Disconnected
            assertEquals(ConnectionState.Disconnected, awaitItem())

            ws.disconnect()
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `malformed JSON message does not crash`() = runTest {
        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onOpen(webSocket: WebSocket, response: Response) {
                    webSocket.send("""{"type":"ready","user_id":"usr-001"}""")
                    webSocket.send("""not valid json at all""")
                    webSocket.send("""{"type":"message_new","channel_id":"chan-001","message":{"id":"msg-1"}}""")
                }
            })
        )

        ws.events.test(timeout = 10.seconds) {
            ws.connect(serverUrl(), TOKEN)

            val ready = awaitItem()
            assertIs<ServerEvent.Ready>(ready)

            val msg = awaitItem()
            assertIs<ServerEvent.MessageNew>(msg)

            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `connect converts https URL to wss scheme`() {
        // Verify URL conversion by connecting to an HTTPS URL; the key is that
        // the request builder accepts the converted URL and enters Connecting.
        ws.connect("https://example.com", TOKEN)
        assertEquals(ConnectionState.Connecting, ws.connectionState.value)
        ws.disconnect()
    }

    @Test
    fun `connect is idempotent while connecting`() = runTest {
        val authLatch = CountDownLatch(1)
        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onMessage(webSocket: WebSocket, text: String) {
                    // Receive Authenticate but never answer with Ready: stay Connecting.
                    authLatch.countDown()
                }
            })
        )

        ws.connect(serverUrl(), TOKEN)
        ws.connect(serverUrl(), TOKEN)

        assertTrue("Server did not receive auth message in time", authLatch.await(5, TimeUnit.SECONDS))

        // The second connect() returned without opening a socket.
        assertEquals(1, mockServer.requestCount)
        assertEquals(ConnectionState.Connecting, ws.connectionState.value)
    }

    @Test
    fun `close 1008 while authenticating reports AuthRejected`() = runTest {
        mockServer.enqueue(
            MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
                override fun onMessage(webSocket: WebSocket, text: String) {
                    if (text.contains("authenticate")) {
                        webSocket.close(1008, "unauthorized")
                    }
                }
            })
        )

        ws.connectionState.test(timeout = 10.seconds) {
            assertEquals(ConnectionState.Disconnected, awaitItem())

            ws.connect(serverUrl(), TOKEN)
            assertEquals(ConnectionState.Connecting, awaitItem())
            assertEquals(ConnectionState.AuthRejected, awaitItem())

            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `handshake 401 reports AuthRejected`() = runTest {
        assertHandshakeStatusReportsAuthRejected(401)
    }

    @Test
    fun `handshake 403 reports AuthRejected`() = runTest {
        assertHandshakeStatusReportsAuthRejected(403)
    }

    @Test
    fun `handshake 500 reports Disconnected`() = runTest {
        mockServer.enqueue(MockResponse().setResponseCode(500))

        ws.connectionState.test(timeout = 10.seconds) {
            assertEquals(ConnectionState.Disconnected, awaitItem())

            ws.connect(serverUrl(), TOKEN)
            assertEquals(ConnectionState.Connecting, awaitItem())
            assertEquals(ConnectionState.Disconnected, awaitItem())

            cancelAndIgnoreRemainingEvents()
        }
    }

    private suspend fun assertHandshakeStatusReportsAuthRejected(code: Int) {
        // The server refuses the upgrade request itself with an HTTP status.
        mockServer.enqueue(MockResponse().setResponseCode(code))

        ws.connectionState.test(timeout = 10.seconds) {
            assertEquals(ConnectionState.Disconnected, awaitItem())

            ws.connect(serverUrl(), TOKEN)
            assertEquals(ConnectionState.Connecting, awaitItem())
            assertEquals(ConnectionState.AuthRejected, awaitItem())

            cancelAndIgnoreRemainingEvents()
        }
    }

    private companion object {
        const val TOKEN = "test-jwt-token"
    }
}
