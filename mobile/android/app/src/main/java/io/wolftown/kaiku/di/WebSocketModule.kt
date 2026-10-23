package io.wolftown.kaiku.di

import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.ws.ConnectivityMonitor
import io.wolftown.kaiku.data.ws.ConnectivitySignal
import io.wolftown.kaiku.data.ws.KaikuWebSocket
import io.wolftown.kaiku.data.ws.RealtimeSocket
import io.wolftown.kaiku.data.ws.WsJson
import kotlinx.serialization.json.Json
import okhttp3.ConnectionSpec
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit
import javax.inject.Qualifier
import javax.inject.Singleton

/** Qualifier to distinguish the WebSocket-specific [Json] from the base [KaikuJson]. */
@Qualifier
@Retention(AnnotationRetention.BINARY)
annotation class WsJsonQualifier

@Module
@InstallIn(SingletonComponent::class)
object WebSocketModule {

    @Provides
    @Singleton
    fun provideOkHttpClient(tls13Spec: ConnectionSpec): OkHttpClient {
        return OkHttpClient.Builder()
            .connectionSpecs(listOf(tls13Spec))
            // Ping/pong is application-level, owned by RealtimeConnectionManager's watchdog.
            .pingInterval(0, TimeUnit.SECONDS)
            .readTimeout(0, TimeUnit.SECONDS)   // No timeout for WebSocket reads
            .build()
    }

    @Provides
    @Singleton
    @WsJsonQualifier
    fun provideWsJson(): Json = WsJson

    @Provides
    @Singleton
    fun provideKaikuWebSocket(
        okHttpClient: OkHttpClient,
        @WsJsonQualifier json: Json
    ): KaikuWebSocket = KaikuWebSocket(okHttpClient, json)

    /** The transport the connection manager drives; same singleton the repositories inject. */
    @Provides
    @Singleton
    fun provideRealtimeSocket(webSocket: KaikuWebSocket): RealtimeSocket = webSocket

    @Provides
    @Singleton
    fun provideConnectivitySignal(monitor: ConnectivityMonitor): ConnectivitySignal = monitor
}
