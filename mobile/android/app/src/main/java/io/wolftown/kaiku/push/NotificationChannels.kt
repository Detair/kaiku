package io.wolftown.kaiku.push

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import android.os.Build
import androidx.annotation.StringRes
import io.wolftown.kaiku.R

/** Notification channel ids and their one-time creation. */
object NotificationChannels {
    const val DM = "dm_messages"
    const val MENTIONS = "mentions"
    const val VOICE = "voice_calls"
    const val SYNC = "background_sync"

    /** Creates all channels; idempotent (re-creating an existing channel is a no-op). */
    fun ensureCreated(context: Context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        val channels = listOf(
            build(context, DM, NotificationManager.IMPORTANCE_HIGH,
                R.string.notif_channel_dm_name, R.string.notif_channel_dm_description),
            build(context, MENTIONS, NotificationManager.IMPORTANCE_HIGH,
                R.string.notif_channel_mentions_name, R.string.notif_channel_mentions_description),
            build(context, VOICE, NotificationManager.IMPORTANCE_LOW,
                R.string.voice_notification_channel_name, R.string.voice_notification_channel_description)
                .apply { setShowBadge(false) },
            build(context, SYNC, NotificationManager.IMPORTANCE_MIN,
                R.string.notif_channel_sync_name, R.string.notif_channel_sync_description)
                .apply { setShowBadge(false) },
        )
        manager.createNotificationChannels(channels)
    }

    private fun build(
        context: Context,
        id: String,
        importance: Int,
        @StringRes nameRes: Int,
        @StringRes descriptionRes: Int,
    ): NotificationChannel =
        NotificationChannel(id, context.getString(nameRes), importance).apply {
            description = context.getString(descriptionRes)
        }
}
