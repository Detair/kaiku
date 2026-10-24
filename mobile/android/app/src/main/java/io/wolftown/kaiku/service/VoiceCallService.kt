package io.wolftown.kaiku.service

import android.app.Notification
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.Manifest
import android.app.ForegroundServiceStartNotAllowedException
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import androidx.annotation.RequiresApi
import androidx.core.app.NotificationCompat
import androidx.core.content.ContextCompat
import dagger.hilt.android.AndroidEntryPoint
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.voice.VoiceServiceEvent
import io.wolftown.kaiku.data.voice.VoiceServiceEvents
import io.wolftown.kaiku.push.NotificationChannels
import java.util.logging.Logger
import javax.inject.Inject

/**
 * Android foreground service that keeps voice audio alive when the app is backgrounded.
 *
 * Uses `FOREGROUND_SERVICE_TYPE_MICROPHONE` to maintain microphone access.
 *
 * Notification includes:
 * - "Mute" toggle action
 * - "Disconnect" action
 * - Tap to open the app
 *
 * Notification actions are delivered as service intents (not broadcasts) so they
 * are handled directly in [onStartCommand].
 */
@AndroidEntryPoint
class VoiceCallService : Service() {

    @Inject lateinit var voiceServiceEvents: VoiceServiceEvents

    override fun onCreate() {
        super.onCreate()  // REQUIRED: Hilt injects fields here
    }

    companion object {
        private val logger = Logger.getLogger("VoiceCallService")
        private const val NOTIFICATION_ID = 1001

        private const val EXTRA_CHANNEL_ID = "channel_id"
        private const val EXTRA_CHANNEL_NAME = "channel_name"
        private const val EXTRA_ACTION = "extra_action"
        const val ACTION_MUTE_TOGGLE = "io.wolftown.kaiku.MUTE_TOGGLE"
        const val ACTION_DISCONNECT = "io.wolftown.kaiku.DISCONNECT"

        /**
         * Starts the foreground voice call service.
         *
         * @param context Application or Activity context
         * @param channelId Voice channel ID
         * @param channelName Display name for the notification
         */
        fun start(context: Context, channelId: String, channelName: String) {
            val intent = Intent(context, VoiceCallService::class.java).apply {
                putExtra(EXTRA_CHANNEL_ID, channelId)
                putExtra(EXTRA_CHANNEL_NAME, channelName)
            }
            context.startForegroundService(intent)
        }

        /**
         * Stops the foreground voice call service.
         */
        fun stop(context: Context) {
            context.stopService(Intent(context, VoiceCallService::class.java))
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Handle notification action intents
        when (intent?.getStringExtra(EXTRA_ACTION)) {
            ACTION_MUTE_TOGGLE -> {
                voiceServiceEvents.emit(VoiceServiceEvent.MuteToggle)
                return START_NOT_STICKY
            }
            ACTION_DISCONNECT -> {
                voiceServiceEvents.emit(VoiceServiceEvent.Disconnect)
                return START_NOT_STICKY
            }
        }

        val channelName = intent?.getStringExtra(EXTRA_CHANNEL_NAME)
            ?: getString(R.string.voice_default_channel_name)

        // A microphone foreground service may only start with RECORD_AUDIO granted
        // (Android 14 throws SecurityException otherwise).
        if (ContextCompat.checkSelfPermission(this, Manifest.permission.RECORD_AUDIO) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            failStart("RECORD_AUDIO not granted")
            return START_NOT_STICKY
        }

        // Channel is created at app start (NotificationChannels.ensureCreated).
        val notification = buildNotification(channelName)

        try {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                startForeground(
                    NOTIFICATION_ID,
                    notification,
                    ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE
                )
            } else {
                startForeground(NOTIFICATION_ID, notification)
            }
        } catch (e: SecurityException) {
            failStart("SecurityException: ${e.message}")
            return START_NOT_STICKY
        } catch (e: IllegalStateException) {
            // ForegroundServiceStartNotAllowedException exists on API 31+ only; the
            // class reference lives in Api31, which never loads on older devices.
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && Api31.isStartNotAllowed(e)) {
                failStart("Foreground start not allowed")
                return START_NOT_STICKY
            }
            throw e
        }

        logger.info("VoiceCallService started for channel: $channelName")
        return START_NOT_STICKY
    }

    private fun failStart(reason: String) {
        logger.warning("VoiceCallService could not start foreground: $reason")
        voiceServiceEvents.emit(VoiceServiceEvent.StartFailed)
        // stopSelf() before startForeground() cancels the pending FGS start timeout.
        stopSelf()
    }

    override fun onDestroy() {
        super.onDestroy()
        logger.info("VoiceCallService destroyed")
    }

    private fun buildNotification(channelName: String): Notification {
        // Tap notification to open the app
        val launchIntent = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            flags = Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP
        }
        val contentPendingIntent = PendingIntent.getActivity(
            this,
            0,
            launchIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        // Mute toggle action — delivered as a service intent
        val muteIntent = Intent(this, VoiceCallService::class.java).apply {
            putExtra(EXTRA_ACTION, ACTION_MUTE_TOGGLE)
        }
        val mutePendingIntent = PendingIntent.getService(
            this,
            1,
            muteIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        // Disconnect action — delivered as a service intent
        val disconnectIntent = Intent(this, VoiceCallService::class.java).apply {
            putExtra(EXTRA_ACTION, ACTION_DISCONNECT)
        }
        val disconnectPendingIntent = PendingIntent.getService(
            this,
            2,
            disconnectIntent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )

        return NotificationCompat.Builder(this, NotificationChannels.VOICE)
            .setContentTitle(getString(R.string.voice_in_channel))
            .setContentText(channelName)
            .setSmallIcon(android.R.drawable.ic_btn_speak_now)
            .setContentIntent(contentPendingIntent)
            .setOngoing(true)
            .setSilent(true)
            .addAction(
                android.R.drawable.ic_lock_silent_mode,
                getString(R.string.voice_mute),
                mutePendingIntent
            )
            .addAction(
                android.R.drawable.ic_menu_close_clear_cancel,
                getString(R.string.voice_disconnect),
                disconnectPendingIntent
            )
            .build()
    }

    @RequiresApi(Build.VERSION_CODES.S)
    private object Api31 {
        fun isStartNotAllowed(e: Throwable): Boolean =
            e is ForegroundServiceStartNotAllowedException
    }
}
