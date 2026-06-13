package io.wolftown.kaiku.ui.dm

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import coil3.compose.AsyncImage
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.DmConversation

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
    viewModel: DmListViewModel = hiltViewModel()
) {
    val conversations by viewModel.conversations.collectAsState()
    val currentUserId by viewModel.currentUserId.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val error by viewModel.error.collectAsState()

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Direct Messages") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back"
                        )
                    }
                }
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
                            text = error ?: "An error occurred",
                            color = MaterialTheme.colorScheme.error,
                            style = MaterialTheme.typography.bodyLarge
                        )
                        Spacer(modifier = Modifier.height(16.dp))
                        Button(onClick = { viewModel.refresh() }) {
                            Text("Retry")
                        }
                    }
                }

                conversations.isEmpty() -> {
                    Text(
                        text = "No direct messages yet",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.align(Alignment.Center)
                    )
                }

                else -> {
                    LazyColumn(modifier = Modifier.fillMaxSize()) {
                        items(conversations, key = { it.id }) { dm ->
                            DmConversationRow(
                                dm = dm,
                                currentUserId = currentUserId,
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
    onClick: () -> Unit
) {
    val title = DmRepository.conversationTitle(dm, currentUserId)
    val avatarUrl = DmRepository.conversationAvatarUrl(dm, currentUserId)
    val hasUnread = dm.unreadCount > 0

    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        AsyncImage(
            model = avatarUrl,
            contentDescription = "$title avatar",
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
            val preview = dm.lastMessage?.let { last ->
                val sender = last.username?.let { "$it: " } ?: ""
                "$sender${last.content}"
            }
            if (preview != null) {
                Text(
                    text = preview,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis
                )
            }
        }

        if (hasUnread) {
            Spacer(modifier = Modifier.width(8.dp))
            Badge {
                Text(text = if (dm.unreadCount > 99) "99+" else dm.unreadCount.toString())
            }
        }
    }
}
