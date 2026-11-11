package io.wolftown.kaiku.ui.channel

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.onLongClick
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.MessageBody
import io.wolftown.kaiku.domain.formatMessageTime
import io.wolftown.kaiku.domain.messageDisplayContent
import io.wolftown.kaiku.domain.reactionSummaries
import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.ui.shared.resolveServerUrl
import java.time.Instant
import java.time.ZoneId

/**
 * Displays a single chat message with author info, content, and metadata.
 *
 * Features:
 * - Author avatar (circular, loaded via Coil) + display name + timestamp
 * - Message content (or a placeholder for end-to-end encrypted messages)
 * - Message time in the local time zone
 * - Reaction chips under the content (tap to toggle your own)
 * - "(edited)" indicator when editedAt is not null
 * - Reply-to indicator when replyTo is not null
 * - Long-press context menu for React/Copy/Edit/Delete; the same actions are
 *   TalkBack actions, and the message has no empty "tap" action (AND-B-11)
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun MessageItem(
    message: Message,
    isOwnMessage: Boolean,
    onEdit: (messageId: String) -> Unit,
    onDelete: (messageId: String) -> Unit,
    onReact: (messageId: String) -> Unit,
    modifier: Modifier = Modifier,
    currentUserId: String? = null,
    /** Called when a reaction chip is tapped; `mine` says you had reacted with it. Display-only when null. */
    onToggleReaction: ((messageId: String, emoji: String, mine: Boolean) -> Unit)? = null,
    /** The message this one answers, when it is loaded (AND-B-22). */
    repliedTo: Message? = null,
    /** Tap on an attachment: open the photo, or the file with the system (AND-B-4). */
    onOpenAttachment: (io.wolftown.kaiku.domain.model.Attachment) -> Unit = {},
) {
    var showContextMenu by remember { mutableStateOf(false) }
    val clipboard = LocalClipboardManager.current
    val context = LocalContext.current
    val copiedText = stringResource(R.string.chat_copied)
    val hasText = message.content.isNotBlank() && !message.encrypted
    val actions = remember(isOwnMessage, hasText) { messageActions(isOwnMessage, hasText) }

    fun run(action: MessageAction) {
        when (action) {
            MessageAction.REACT -> onReact(message.id)
            MessageAction.COPY -> {
                clipboard.setText(AnnotatedString(plainMessageText(message.content)))
                // Android 13+ confirms copies itself.
                if (android.os.Build.VERSION.SDK_INT < 33) {
                    android.widget.Toast.makeText(context, copiedText, android.widget.Toast.LENGTH_SHORT).show()
                }
            }
            MessageAction.EDIT -> onEdit(message.id)
            MessageAction.DELETE -> onDelete(message.id)
        }
    }

    val actionLabels = actions.associateWith { stringResource(it.labelRes) }
    val menuLabel = stringResource(R.string.chat_message_actions)

    Column(
        modifier = modifier
            .fillMaxWidth()
            // Long press opens the menu; a plain tap does nothing, so the message
            // is not announced as "double tap to activate".
            .pointerInput(Unit) {
                detectTapGestures(onLongPress = { showContextMenu = true })
            }
            .semantics {
                onLongClick(label = menuLabel) { showContextMenu = true; true }
                customActions = actions.map { action ->
                    CustomAccessibilityAction(actionLabels.getValue(action)) { run(action); true }
                }
            }
            .padding(horizontal = 16.dp, vertical = 4.dp)
    ) {
        // Reply indicator: who and what is answered, when that message is loaded.
        if (message.replyTo != null) {
            val quote = repliedTo?.let { replyQuote(it) }
            Text(
                text = if (quote == null) {
                    stringResource(R.string.chat_replying_to)
                } else {
                    stringResource(
                        R.string.chat_replying_to_named,
                        quote.author,
                        quote.snippet.ifEmpty { stringResource(R.string.chat_pending_attachment) },
                    )
                },
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(start = 44.dp, bottom = 2.dp)
            )
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.Start
        ) {
            // Avatar: decorative, the name follows (TalkBack does not read "<name>'s avatar"
            // before every message). Without a picture the first letter of the name shows.
            Box(
                modifier = Modifier
                    .size(36.dp)
                    .clip(CircleShape)
                    .background(MaterialTheme.colorScheme.primaryContainer),
                contentAlignment = Alignment.Center,
            ) {
                Text(
                    text = message.author.displayName.trim().take(1).uppercase(),
                    style = MaterialTheme.typography.titleSmall,
                    color = MaterialTheme.colorScheme.onPrimaryContainer,
                )
                val avatar = resolveServerUrl(message.author.avatarUrl)
                if (avatar != null) {
                    AsyncImage(
                        model = avatar,
                        contentDescription = null,
                        modifier = Modifier.matchParentSize()
                    )
                }
            }

            Spacer(modifier = Modifier.width(8.dp))

            Column(modifier = Modifier.weight(1f)) {
                // Author name + timestamp
                Row(
                    verticalAlignment = Alignment.CenterVertically
                ) {
                    Text(
                        text = message.author.displayName,
                        style = MaterialTheme.typography.bodyMedium,
                        fontWeight = FontWeight.SemiBold,
                        color = MaterialTheme.colorScheme.onSurface
                    )

                    Spacer(modifier = Modifier.width(8.dp))

                    val formattedTime = remember(message.createdAt) {
                        formatMessageTime(message.createdAt, ZoneId.systemDefault(), Instant.now())
                    }
                    Text(
                        text = formattedTime,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                }

                // Message content (inline markdown: bold/italic/strike/code/spoiler)
                when (val body = messageDisplayContent(message)) {
                    is MessageBody.Text -> CompositionLocalProvider(
                        LocalTextStyle provides MaterialTheme.typography.bodyMedium.copy(
                            color = MaterialTheme.colorScheme.onSurface,
                        )
                    ) {
                        MessageContent(content = body.markdown)
                    }

                    MessageBody.EncryptedPlaceholder -> Text(
                        text = stringResource(R.string.chat_message_encrypted_placeholder),
                        style = MaterialTheme.typography.bodyMedium,
                        fontStyle = FontStyle.Italic,
                        color = MaterialTheme.colorScheme.onSurfaceVariant
                    )
                }

                // Attachments (image thumbnails + file chips)
                MessageAttachments(attachments = message.attachments, onOpen = onOpenAttachment)

                // Reactions
                val reactions = remember(message.reactions, currentUserId) {
                    reactionSummaries(message, currentUserId)
                }
                if (reactions.isNotEmpty()) {
                    Row(
                        modifier = Modifier
                            .horizontalScroll(rememberScrollState())
                            .padding(top = 4.dp),
                        horizontalArrangement = Arrangement.spacedBy(6.dp)
                    ) {
                        reactions.forEach { reaction ->
                            val description = stringResource(
                                R.string.chat_reaction_chip_description,
                                reaction.emoji,
                                reaction.count
                            )
                            FilterChip(
                                selected = reaction.mine,
                                onClick = {
                                    onToggleReaction?.invoke(message.id, reaction.emoji, reaction.mine)
                                },
                                enabled = onToggleReaction != null,
                                label = {
                                    Text(
                                        stringResource(
                                            R.string.chat_reaction_chip_label,
                                            reaction.emoji,
                                            reaction.count
                                        )
                                    )
                                },
                                modifier = Modifier.semantics { contentDescription = description }
                            )
                        }
                    }
                }

                // Edited indicator
                if (message.editedAt != null) {
                    Text(
                        text = stringResource(R.string.chat_message_edited_badge),
                        style = MaterialTheme.typography.labelSmall,
                        fontStyle = FontStyle.Italic,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(top = 2.dp)
                    )
                }
            }
        }

        // Context menu dropdown
        DropdownMenu(
            expanded = showContextMenu,
            onDismissRequest = { showContextMenu = false }
        ) {
            actions.forEach { action ->
                DropdownMenuItem(
                    text = { Text(actionLabels.getValue(action)) },
                    onClick = {
                        showContextMenu = false
                        run(action)
                    }
                )
            }
        }
    }
}

/** What can be done with a message, in menu order (also the TalkBack actions). */
enum class MessageAction(@androidx.annotation.StringRes val labelRes: Int) {
    REACT(R.string.chat_react),
    COPY(R.string.chat_copy_text),
    EDIT(R.string.chat_edit),
    DELETE(R.string.chat_delete),
}

/** React for everyone, Copy when there is text, Edit and Delete on your own messages. */
fun messageActions(isOwnMessage: Boolean, hasText: Boolean): List<MessageAction> = buildList {
    add(MessageAction.REACT)
    if (hasText) add(MessageAction.COPY)
    if (isOwnMessage) {
        if (hasText) add(MessageAction.EDIT)
        add(MessageAction.DELETE)
    }
}

/** Who wrote the answered message and how it starts (AND-B-22). */
data class ReplyQuote(val author: String, val snippet: String)

private const val REPLY_SNIPPET_MAX = 80

fun replyQuote(original: Message): ReplyQuote {
    val text = if (original.encrypted) "" else plainMessageText(original.content)
        .replace(Regex("\\s+"), " ").trim()
    val snippet = if (text.length > REPLY_SNIPPET_MAX) text.take(REPLY_SNIPPET_MAX - 1).trimEnd() + "…" else text
    return ReplyQuote(original.author.displayName, snippet)
}
