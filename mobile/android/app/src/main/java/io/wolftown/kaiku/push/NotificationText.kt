package io.wolftown.kaiku.push

import androidx.annotation.StringRes
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.model.Message

/** A notification line's text: the message text itself or a localized placeholder. */
sealed interface LineText {
    data class Raw(val text: String) : LineText
    data class Res(@StringRes val id: Int) : LineText
}

/** What a message notification shows for one message. */
object NotificationText {
    const val MAX_LENGTH = 300

    fun forMessage(m: Message): LineText = when {
        m.encrypted -> LineText.Res(R.string.notif_encrypted_message)
        m.content.isBlank() && m.attachments.any { it.mimeType.startsWith("image/") } ->
            LineText.Res(R.string.notif_sent_photo)
        m.content.isBlank() && m.attachments.isNotEmpty() -> LineText.Res(R.string.notif_sent_file)
        else -> LineText.Raw(m.content.take(MAX_LENGTH))
    }
}

/** One message in a channel notification. */
data class NotificationLine(
    val messageId: String,
    val senderName: String,
    val text: LineText,
    val timestampMs: Long,
)

/**
 * The new messages of one channel for account [userId]. [title] is the
 * conversation title, or null when it could not be resolved (the presenter
 * then shows a generic title).
 */
data class ChannelNotification(
    val userId: String,
    val channelId: String,
    val title: String?,
    val isDm: Boolean,
    val kind: WakeKind,
    val lines: List<NotificationLine>,
)

/** Result of [NotificationSink.show]. */
enum class ShowResult {
    POSTED,

    /** [NotificationSink.show]'s `prepare` returned null. */
    REFUSED,

    /** A notification was ready, but the user disabled notifications. */
    DISABLED,
}

/** Posts and clears per-channel message notifications. */
interface NotificationSink {
    /**
     * Posts the notification [prepare] returns, or nothing when it returns
     * null. [prepare] runs inside the same critical section as the post and
     * as [cancel]/[cancelAll], immediately before posting, so it can make the
     * final checks (session, visibility, read state) atomically with them. It
     * also runs when notifications are disabled, so the caller's bookkeeping
     * is the same either way.
     */
    fun show(prepare: () -> ChannelNotification?): ShowResult

    fun cancel(channelId: String)

    /** Removes every message notification and the group summary (logout, account change). */
    fun cancelAll()
}
