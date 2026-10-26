package io.wolftown.kaiku.push

import android.Manifest
import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.service.notification.StatusBarNotification
import androidx.core.app.NotificationCompat
import androidx.core.app.Person
import androidx.core.content.ContextCompat
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.R
import io.wolftown.kaiku.navigation.DeepLinks
import io.wolftown.kaiku.ui.MainActivity
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Posts one MessagingStyle notification per channel (tag = channel id, id
 * [MESSAGE_ID]) in the [GROUP] group with a summary (id [SUMMARY_ID]).
 * New messages are appended to an already shown notification, keeping the
 * last [MAX_LINES] lines. The lock screen shows only a generic public version.
 */
@Singleton
class AndroidNotificationPresenter @Inject constructor(
    @ApplicationContext private val context: Context,
) : NotificationSink {

    companion object {
        private val logger = Logger.getLogger("NotificationPresenter")

        const val GROUP = "io.wolftown.kaiku.MESSAGES"
        const val MESSAGE_ID = 1
        const val SUMMARY_ID = 0
        const val MAX_LINES = 7

        /** Account a message notification was posted for. */
        const val EXTRA_USER_ID = "io.wolftown.kaiku.USER_ID"
    }

    private val manager: NotificationManager?
        get() = context.getSystemService(NotificationManager::class.java)

    @Synchronized
    override fun show(prepare: () -> ChannelNotification?): ShowResult {
        // Final checks and the post form one critical section with cancel/cancelAll.
        val n = prepare() ?: return ShowResult.REFUSED
        val manager = manager
        if (manager == null || !canPost(manager)) {
            logger.info("message notification skipped: notifications disabled")
            return ShowResult.DISABLED
        }
        manager.notify(n.channelId, MESSAGE_ID, build(manager, n))
        manager.notify(SUMMARY_ID, summary())
        return ShowResult.POSTED
    }

    private fun build(manager: NotificationManager, n: ChannelNotification): Notification {
        val channel = if (n.isDm) NotificationChannels.DM else NotificationChannels.MENTIONS
        val title = n.title ?: context.getString(R.string.notif_new_message)

        val newMessages = n.lines.map { line ->
            NotificationCompat.MessagingStyle.Message(
                resolve(line.text),
                line.timestampMs,
                Person.Builder().setName(line.senderName).build(),
            )
        }
        val kept = (existingMessages(manager, n) + newMessages).takeLast(MAX_LINES)
        val style = NotificationCompat.MessagingStyle(
            Person.Builder().setName(context.getString(R.string.voice_participants_you)).build()
        )
            .setConversationTitle(title)
            // A 1:1 DM shows the sender as the title; everything else shows the conversation title.
            .setGroupConversation(!(n.isDm && n.lines.all { it.senderName == title }))
        kept.forEach { style.addMessage(it) }

        val publicVersion = NotificationCompat.Builder(context, channel)
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setContentTitle(context.getString(R.string.notif_new_message))
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .build()

        return NotificationCompat.Builder(context, channel)
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setStyle(style)
            .setContentTitle(title)
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(true)
            .setGroup(GROUP)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(publicVersion)
            .setContentIntent(channelIntent(n.channelId))
            .setWhen(n.lines.maxOfOrNull { it.timestampMs } ?: System.currentTimeMillis())
            .setShowWhen(true)
            .addExtras(Bundle().apply { putString(EXTRA_USER_ID, n.userId) })
            .build()
    }

    @Synchronized
    override fun cancel(channelId: String) {
        val manager = manager ?: return
        manager.cancel(channelId, MESSAGE_ID)
        val othersRemain = messageNotifications(manager).any { it.tag != channelId }
        if (!othersRemain) manager.cancel(SUMMARY_ID)
    }

    @Synchronized
    override fun cancelAll() {
        val manager = manager ?: return
        messageNotifications(manager).forEach { manager.cancel(it.tag, MESSAGE_ID) }
        manager.cancel(SUMMARY_ID)
    }

    private fun messageNotifications(manager: NotificationManager): List<StatusBarNotification> =
        manager.activeNotifications.filter {
            it.id == MESSAGE_ID && it.tag != null && it.notification.group == GROUP
        }

    /**
     * Lines of the channel's notification that is still shown, if it was
     * posted for the same account; a foreign one is replaced, never extended.
     */
    private fun existingMessages(
        manager: NotificationManager,
        n: ChannelNotification,
    ): List<NotificationCompat.MessagingStyle.Message> {
        val active = manager.activeNotifications.firstOrNull { it.tag == n.channelId && it.id == MESSAGE_ID }
            ?: return emptyList()
        if (active.notification.extras.getString(EXTRA_USER_ID) != n.userId) return emptyList()
        val style = NotificationCompat.MessagingStyle.extractMessagingStyleFromNotification(active.notification)
            ?: return emptyList()
        return style.messages.toList()
    }

    private fun canPost(manager: NotificationManager): Boolean {
        if (!manager.areNotificationsEnabled()) return false
        return Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
    }

    private fun resolve(text: LineText): CharSequence = when (text) {
        is LineText.Raw -> text.text
        is LineText.Res -> context.getString(text.id)
    }

    private fun channelIntent(channelId: String): PendingIntent {
        val intent = Intent(
            Intent.ACTION_VIEW,
            Uri.parse(DeepLinks.channelUri(channelId)),
            context,
            MainActivity::class.java,
        )
        return PendingIntent.getActivity(
            context,
            channelId.hashCode(),
            intent,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    /** The summary lives on a fixed channel; it never alerts (GROUP_ALERT_CHILDREN). */
    private fun summary(): Notification {
        val launch = PendingIntent.getActivity(
            context,
            SUMMARY_ID,
            Intent(context, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return NotificationCompat.Builder(context, NotificationChannels.MENTIONS)
            .setSmallIcon(android.R.drawable.stat_notify_chat)
            .setContentTitle(context.getString(R.string.notif_new_message))
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .setGroup(GROUP)
            .setGroupSummary(true)
            .setGroupAlertBehavior(NotificationCompat.GROUP_ALERT_CHILDREN)
            .setAutoCancel(true)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setContentIntent(launch)
            .build()
    }
}
