package io.wolftown.kaiku.ui.channel

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
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
 * - Long-press context menu for Edit/Delete/React actions
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
    onToggleReaction: ((messageId: String, emoji: String, mine: Boolean) -> Unit)? = null
) {
    var showContextMenu by remember { mutableStateOf(false) }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .combinedClickable(
                onClick = {},
                onLongClick = { showContextMenu = true }
            )
            .padding(horizontal = 16.dp, vertical = 4.dp)
    ) {
        // Reply indicator
        if (message.replyTo != null) {
            Text(
                text = stringResource(R.string.chat_replying_to),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.primary,
                modifier = Modifier.padding(start = 48.dp, bottom = 2.dp)
            )
        }

        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.Start
        ) {
            // Avatar
            AsyncImage(
                model = resolveServerUrl(message.author.avatarUrl),
                contentDescription = stringResource(
                    R.string.common_avatar_description,
                    message.author.displayName
                ),
                modifier = Modifier
                    .size(36.dp)
                    .clip(CircleShape)
            )

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
                MessageAttachments(attachments = message.attachments)

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
                                label = { Text("${reaction.emoji} ${reaction.count}") },
                                modifier = Modifier.semantics { contentDescription = description }
                            )
                        }
                    }
                }

                // Edited indicator
                if (message.editedAt != null) {
                    Text(
                        text = stringResource(R.string.chat_edited),
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
            DropdownMenuItem(
                text = { Text(stringResource(R.string.chat_react)) },
                onClick = {
                    showContextMenu = false
                    onReact(message.id)
                }
            )
            if (isOwnMessage) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.chat_edit)) },
                    onClick = {
                        showContextMenu = false
                        onEdit(message.id)
                    }
                )
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.chat_delete)) },
                    onClick = {
                        showContextMenu = false
                        onDelete(message.id)
                    }
                )
            }
        }
    }
}
