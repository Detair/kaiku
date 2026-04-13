package io.wolftown.kaiku.ui.channel

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import coil3.compose.AsyncImage
import io.wolftown.kaiku.domain.model.Message

/**
 * Displays a single chat message with author info, content, and metadata.
 *
 * Features:
 * - Author avatar (circular, loaded via Coil) + display name + timestamp
 * - Message content
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
    modifier: Modifier = Modifier
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
                text = "Replying to a message",
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
                model = message.author.avatarUrl,
                contentDescription = "${message.author.displayName}'s avatar",
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
                        formatTimestamp(message.createdAt)
                    }
                    Text(
                        text = formattedTime,
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis
                    )
                }

                // Message content
                Text(
                    text = message.content,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.padding(top = 2.dp)
                )

                // Edited indicator
                if (message.editedAt != null) {
                    Text(
                        text = "(edited)",
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
                text = { Text("React") },
                onClick = {
                    showContextMenu = false
                    onReact(message.id)
                }
            )
            if (isOwnMessage) {
                DropdownMenuItem(
                    text = { Text("Edit") },
                    onClick = {
                        showContextMenu = false
                        onEdit(message.id)
                    }
                )
                DropdownMenuItem(
                    text = { Text("Delete") },
                    onClick = {
                        showContextMenu = false
                        onDelete(message.id)
                    }
                )
            }
        }
    }
}

/**
 * Formats an ISO 8601 timestamp into a short display format.
 * Shows time only (HH:mm) for simplicity.
 */
private fun formatTimestamp(iso8601: String): String {
    if (iso8601.isBlank()) return ""
    return try {
        // Extract time portion from ISO 8601: "2026-03-12T10:30:00Z" -> "10:30"
        val timeIndex = iso8601.indexOf('T')
        if (timeIndex == -1) return iso8601
        val timePart = iso8601.substring(timeIndex + 1)
        val colonIndex = timePart.indexOf(':', timePart.indexOf(':') + 1)
        if (colonIndex == -1) timePart.take(5) else timePart.take(colonIndex)
    } catch (_: Exception) {
        iso8601
    }
}
