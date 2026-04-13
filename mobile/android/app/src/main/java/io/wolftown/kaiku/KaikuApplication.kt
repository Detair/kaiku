package io.wolftown.kaiku

import android.app.Application
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
        } catch (e: Exception) {
            logger.warning("Failed to install security provider: ${e.message}")
        }
    }
}
