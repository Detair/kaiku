package io.wolftown.kaiku

import android.app.Application
import com.google.android.gms.common.GoogleApiAvailability
import com.google.android.gms.common.GooglePlayServicesNotAvailableException
import com.google.android.gms.common.GooglePlayServicesRepairableException
import com.google.android.gms.security.ProviderInstaller
import dagger.hilt.android.HiltAndroidApp
import java.util.logging.Logger

@HiltAndroidApp
class KaikuApplication : Application() {
    private val logger = Logger.getLogger("KaikuApplication")

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
}
