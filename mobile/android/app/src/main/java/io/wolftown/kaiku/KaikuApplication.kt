package io.wolftown.kaiku

import android.app.Application
import coil3.ImageLoader
import coil3.PlatformContext
import coil3.SingletonImageLoader
import coil3.network.okhttp.OkHttpNetworkFetcherFactory
import com.google.android.gms.common.GoogleApiAvailability
import com.google.android.gms.common.GooglePlayServicesNotAvailableException
import com.google.android.gms.common.GooglePlayServicesRepairableException
import com.google.android.gms.security.ProviderInstaller
import dagger.hilt.android.HiltAndroidApp
import io.wolftown.kaiku.data.api.AuthHeaderInterceptor
import io.wolftown.kaiku.data.local.TokenStorage
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit
import java.util.logging.Logger
import javax.inject.Inject

@HiltAndroidApp
class KaikuApplication : Application(), SingletonImageLoader.Factory {
    private val logger = Logger.getLogger("KaikuApplication")

    /** The app's TLS 1.3 client (see WebSocketModule); shared so images reuse its connection pool. */
    @Inject
    lateinit var okHttpClient: OkHttpClient

    @Inject
    lateinit var tokenStorage: TokenStorage

    override fun onCreate() {
        super.onCreate()
        try {
            ProviderInstaller.installIfNeeded(this)
        } catch (e: GooglePlayServicesRepairableException) {
            logger.severe("Play Services needs update for TLS 1.3: ${e.message}")
            GoogleApiAvailability.getInstance()
                .showErrorNotification(this, e.connectionStatusCode)
        } catch (e: GooglePlayServicesNotAvailableException) {
            logger.severe("Play Services not available — TLS 1.3 unsupported, network will fail: ${e.message}")
        } catch (e: Throwable) {
            // Defensive fallback — don't crash app startup on unexpected provider errors.
            // Network calls will fail later if TLS 1.3 was the issue.
            logger.severe("Unexpected error installing security provider: ${e.javaClass.simpleName}: ${e.message}")
        }
    }

    /**
     * Coil's singleton image loader. Images are fetched with the shared TLS 1.3
     * [OkHttpClient] plus [AuthHeaderInterceptor], which adds the bearer token
     * only for requests to the configured server. The interceptor is an
     * application interceptor (`addInterceptor`), so OkHttp strips the
     * header if the server redirects to another host.
     */
    override fun newImageLoader(context: PlatformContext): ImageLoader {
        return ImageLoader.Builder(context)
            .components {
                add(
                    OkHttpNetworkFetcherFactory(
                        callFactory = {
                            okHttpClient.newBuilder()
                                // The shared client disables read timeouts for the
                                // WebSocket; image downloads must not hang forever.
                                .readTimeout(IMAGE_READ_TIMEOUT_SECONDS, TimeUnit.SECONDS)
                                .addInterceptor(AuthHeaderInterceptor(tokenStorage))
                                .build()
                        }
                    )
                )
            }
            .build()
    }

    private companion object {
        const val IMAGE_READ_TIMEOUT_SECONDS = 30L
    }
}
