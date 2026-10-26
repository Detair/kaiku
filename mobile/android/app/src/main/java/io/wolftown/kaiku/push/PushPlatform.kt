package io.wolftown.kaiku.push

import android.content.Context
import android.os.Build
import androidx.core.app.NotificationManagerCompat
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.firebase.FirebaseApp
import dagger.hilt.android.qualifiers.ApplicationContext
import org.unifiedpush.android.connector.UnifiedPush
import java.util.logging.Logger
import javax.inject.Inject

/** Device facts the push registration depends on (an interface so tests need no Android). */
interface PushPlatform {
    fun notificationsAllowed(): Boolean
    fun gmsAvailable(): Boolean

    /** Firebase project baked into the APK, or null when built without `google-services.json`. */
    fun firebaseProjectId(): String?

    /** Installed UnifiedPush distributors (package names). */
    fun distributors(): List<String>
    fun preferUnifiedPush(): Boolean
    fun savedDistributor(): String?

    /** Remembers the distributor the selector settled on, for the UnifiedPush transport. */
    fun saveDistributor(distributor: String) {}

    /** Shown in the server's device list (C19 `device_label`, at most 64 characters). */
    fun deviceLabel(): String?
}

class AndroidPushPlatform @Inject constructor(
    @ApplicationContext private val context: Context,
    private val preferences: PushPreferences,
) : PushPlatform {

    private companion object {
        val logger: Logger = Logger.getLogger("AndroidPushPlatform")
        const val MAX_LABEL_LENGTH = 64
    }

    override fun notificationsAllowed(): Boolean =
        NotificationManagerCompat.from(context).areNotificationsEnabled()

    override fun gmsAvailable(): Boolean = try {
        GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(context) == ConnectionResult.SUCCESS
    } catch (e: Exception) {
        logger.info("Play services check failed: ${e.javaClass.simpleName}")
        false
    }

    override fun firebaseProjectId(): String? = try {
        // Empty when the APK was built without google-services.json: FirebaseApp never initialised.
        FirebaseApp.getApps(context).firstOrNull()?.options?.projectId?.takeIf { it.isNotBlank() }
    } catch (e: Exception) {
        logger.info("Firebase lookup failed: ${e.javaClass.simpleName}")
        null
    }

    override fun distributors(): List<String> = try {
        UnifiedPush.getDistributors(context)
    } catch (e: Exception) {
        logger.info("Distributor lookup failed: ${e.javaClass.simpleName}")
        emptyList()
    }

    override fun preferUnifiedPush(): Boolean = preferences.preferUnifiedPush

    override fun savedDistributor(): String? = preferences.distributor

    override fun saveDistributor(distributor: String) {
        if (preferences.distributor != distributor) preferences.distributor = distributor
    }

    override fun deviceLabel(): String? =
        Build.MODEL?.trim()?.take(MAX_LABEL_LENGTH)?.takeIf { it.isNotEmpty() }
}
