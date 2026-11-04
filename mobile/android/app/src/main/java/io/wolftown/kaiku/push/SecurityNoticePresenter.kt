package io.wolftown.kaiku.push

import android.app.Notification
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.annotation.StringRes
import androidx.core.app.NotificationCompat
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.keydevice.KeyDeviceRepository
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.navigation.DeepLinks
import io.wolftown.kaiku.ui.MainActivity
import io.wolftown.kaiku.ui.security.NoticeText
import java.util.logging.Logger
import javax.inject.Inject
import javax.inject.Singleton

/** What a security notification shows; built without Android so it is tested on the JVM. */
data class SecurityNotification(
    val notificationId: Int,
    val channelId: String,
    @StringRes val text: Int,
    @StringRes val subject: Int,
    val count: Int,
    @StringRes val publicText: Int,
    val visibilityPrivate: Boolean,
    /** Always empty: no notification action approves or decides anything (B.1). */
    val actions: List<String>,
    val tapUri: String,
)

fun interface SecurityNotificationSink {
    fun post(n: SecurityNotification)
}

/**
 * C28 `t=security` notifications (L2-A6): title "Kaiku", text from
 * [NoticeText.push] with the subject ("your account" or another account),
 * `VISIBILITY_PRIVATE` with a public "Security notice", no actions, tap opens
 * the security log. One notification per notice (updated in place). Not gated
 * by Kaiku's notification settings, the message coalescer or a session.
 */
@Singleton
class SecurityNoticePresenter internal constructor(
    private val sink: SecurityNotificationSink,
    private val ownUserId: () -> String?,
) {
    /** Signed out, a key phone still knows its account (it keeps its push subscription, C19 am. 3). */
    @Inject
    constructor(sink: AndroidSecurityNotificationSink, tokenStorage: TokenStorage, keyDevices: KeyDeviceRepository) :
        this(sink, ownUserId = { tokenStorage.getUserId() ?: keyDevices.local()?.userId })

    fun present(w: SecurityWake) {
        sink.post(
            SecurityNotification(
                notificationId = ("security-" + w.noticeId).hashCode(),
                channelId = NotificationChannels.SECURITY,
                text = NoticeText.push(w.eventType),
                // "Your account" only when this phone knows it is the reader's own.
                subject = if (ownUserId().let { it != null && it == w.subjectId }) {
                    R.string.security_subject_yours
                } else {
                    R.string.security_subject_other
                },
                count = w.count,
                publicText = R.string.security_push_public,
                visibilityPrivate = true,
                actions = emptyList(),
                tapUri = DeepLinks.SECURITY_URI,
            ),
        )
    }
}

class AndroidSecurityNotificationSink @Inject constructor(
    @ApplicationContext private val context: Context,
) : SecurityNotificationSink {

    private companion object {
        val logger: Logger = Logger.getLogger("SecurityNotices")
    }

    override fun post(n: SecurityNotification) {
        NotificationChannels.ensureCreated(context)
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        if (!manager.areNotificationsEnabled()) {
            logger.info("security notice not shown: notifications are off for the app")
            return
        }
        val subject = context.getString(n.subject)
        val text = context.getString(n.text, subject, "", "", n.count)
        val tap = PendingIntent.getActivity(
            context,
            n.notificationId,
            Intent(context, MainActivity::class.java).setData(Uri.parse(n.tapUri)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val public = NotificationCompat.Builder(context, n.channelId)
            .setSmallIcon(android.R.drawable.ic_lock_idle_lock)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(context.getString(n.publicText))
            .build()
        val notification = NotificationCompat.Builder(context, n.channelId)
            .setSmallIcon(android.R.drawable.ic_lock_idle_lock)
            .setContentTitle(context.getString(R.string.app_name))
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setCategory(Notification.CATEGORY_STATUS)
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(public)
            .setContentIntent(tap)
            .setAutoCancel(true)
            .setOnlyAlertOnce(false)
            .build()
        try {
            manager.notify(n.notificationId, notification)
        } catch (e: SecurityException) {
            logger.info("security notice not shown: ${e.javaClass.simpleName}")
        }
    }
}
