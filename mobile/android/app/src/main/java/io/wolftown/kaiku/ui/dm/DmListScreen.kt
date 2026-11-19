package io.wolftown.kaiku.ui.dm

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Lock
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import coil3.compose.AsyncImage
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.AttachmentKind
import io.wolftown.kaiku.domain.PreviewBody
import io.wolftown.kaiku.domain.PreviewSender
import io.wolftown.kaiku.domain.formatListTime
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.previewBody
import io.wolftown.kaiku.domain.previewSender
import java.time.Instant
import java.time.ZoneId
import io.wolftown.kaiku.ui.shared.resolveServerUrl

/**
 * Direct-message conversation list.
 *
 * Each row shows the conversation avatar, its title (the other participant for
 * a 1:1 DM, or the group name), a one-line last-message preview, and an unread
 * badge. Tapping a row opens the DM, which is just a channel — [onOpenDm]
 * navigates to the existing text channel screen by channel id.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DmListScreen(
    onOpenDm: (channelId: String) -> Unit,
    onNavigateBack: () -> Unit,
    onScan: () -> Unit = {},
    onNewMessage: () -> Unit = {},
    viewModel: DmListViewModel = hiltViewModel()
) {
    val conversations by viewModel.conversations.collectAsState()
    val currentUserId by viewModel.currentUserId.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val error by viewModel.error.collectAsState()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.dm_title)) },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.common_back)
                        )
                    }
                },
                actions = {
                    TextButton(onClick = onScan) {
                        Text(stringResource(R.string.scan_action), color = MaterialTheme.colorScheme.onSurface)
                    }
                }
            )
        },
        // Labelled, not an icon alone: "New message" must be found without guessing.
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = onNewMessage,
                icon = { Icon(Icons.Filled.Edit, contentDescription = null) },
                text = { Text(stringResource(R.string.dm_new_title)) },
            )
        }
    ) { paddingValues ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
        ) {
            when {
                isLoading && conversations.isEmpty() -> {
                    CircularProgressIndicator(modifier = Modifier.align(Alignment.Center))
                }

                error != null && conversations.isEmpty() -> {
                    Column(
                        modifier = Modifier.align(Alignment.Center),
                        horizontalAlignment = Alignment.CenterHorizontally
                    ) {
                        Text(
                            text = stringResource(error ?: R.string.common_error_generic),
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodyLarge
                        )
                        Spacer(modifier = Modifier.height(16.dp))
                        Button(onClick = { viewModel.refresh() }) {
                            Text(stringResource(R.string.common_retry))
                        }
                    }
                }

                conversations.isEmpty() -> {
                    Text(
                        text = stringResource(R.string.dm_empty),
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.align(Alignment.Center)
                    )
                }

                else -> {
                    // Bottom space so the "New message" button never covers the last row.
                    val now = rememberMinuteClock()
                    LazyColumn(modifier = Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 88.dp)) {
                        items(conversations, key = { it.id }) { dm ->
                            DmConversationRow(
                                dm = dm,
                                currentUserId = currentUserId,
                                now = now,
                                onClick = {
                                    viewModel.onOpenConversation(dm.id)
                                    onOpenDm(dm.id)
                                }
                            )
                            HorizontalDivider()
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DmConversationRow(
    dm: DmConversation,
    currentUserId: String,
    now: Instant,
    onClick: () -> Unit
) {
    // C28: the Kaiku DM, recognised by its flags only, has a built-in icon and a fixed name.
    val isKaiku = dm.isSystem && dm.readOnly
    val title = if (isKaiku) stringResource(R.string.security_kaiku_dm_name) else DmRepository.conversationTitle(dm, currentUserId)
    val avatarUrl = resolveServerUrl(DmRepository.conversationAvatarUrl(dm, currentUserId))
    val hasUnread = dm.unreadCount > 0

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 72.dp)
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        if (isKaiku) {
            Icon(
                imageVector = Icons.Filled.Lock,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.size(44.dp),
            )
        } else AsyncImage(
            model = avatarUrl,
            contentDescription = stringResource(R.string.common_avatar_description, title),
            modifier = Modifier
                .size(44.dp)
                .clip(CircleShape)
        )

        Spacer(modifier = Modifier.width(12.dp))

        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = title,
                style = MaterialTheme.typography.bodyLarge,
                fontWeight = if (hasUnread) FontWeight.SemiBold else FontWeight.Normal,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
            val preview = if (isKaiku) stringResource(R.string.security_kaiku_dm_subtitle) else conversationPreview(dm, currentUserId)
            Text(
                text = preview,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis
            )
        }

        // Time above the unread badge; no fixed sizes, so a large font grows the row.
        Spacer(modifier = Modifier.width(8.dp))
        Column(horizontalAlignment = Alignment.End) {
            val time = formatListTime(
                dm.lastMessage?.createdAt ?: dm.createdAt,
                ZoneId.systemDefault(),
                now,
                LocalConfiguration.current.locales[0],
                use24Hour = android.text.format.DateFormat.is24HourFormat(LocalContext.current),
            )
            if (time.isNotEmpty()) {
                Text(
                    text = time,
                    style = MaterialTheme.typography.labelMedium,
                    color = if (hasUnread) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    softWrap = false,
                )
            }
            if (hasUnread) {
                Spacer(modifier = Modifier.height(4.dp))
                Badge {
                    Text(
                        text = if (dm.unreadCount > 99) {
                            stringResource(R.string.dm_unread_overflow)
                        } else {
                            dm.unreadCount.toString()
                        }
                    )
                }
            }
        }
    }
}

/** "You: ok", "Anna: ok" (groups), "ok" (1:1), or the attachment's kind for a message without text. */
@Composable
internal fun conversationPreview(dm: DmConversation, currentUserId: String): String {
    val last = dm.lastMessage ?: return stringResource(R.string.dm_preview_no_messages)
    val body = when (val b = previewBody(last)) {
        is PreviewBody.Text -> b.text
        is PreviewBody.Attached -> stringResource(
            when (b.kind) {
                AttachmentKind.PHOTO -> R.string.dm_preview_photo
                AttachmentKind.VIDEO -> R.string.dm_preview_video
                AttachmentKind.AUDIO -> R.string.dm_preview_audio
                AttachmentKind.FILE -> R.string.dm_preview_file
            }
        )
        PreviewBody.SomeAttachment -> stringResource(R.string.dm_preview_attachment)
        PreviewBody.Encrypted -> stringResource(R.string.dm_preview_encrypted)
    }
    return when (val sender = previewSender(dm, last, currentUserId)) {
        PreviewSender.Own -> stringResource(R.string.dm_preview_own, body)
        is PreviewSender.Named -> stringResource(R.string.dm_last_message_preview, sender.name, body)
        PreviewSender.Hidden -> body
    }
}

/**
 * The current time, renewed at every full minute while the list is shown,
 * so "today" (time) turns into a date after midnight without a reload.
 */
@Composable
private fun rememberMinuteClock(): Instant {
    val now by produceState(Instant.now()) {
        while (true) {
            val next = (System.currentTimeMillis() / 60_000L + 1) * 60_000L
            kotlinx.coroutines.delay(next - System.currentTimeMillis())
            value = Instant.now()
        }
    }
    return now
}
