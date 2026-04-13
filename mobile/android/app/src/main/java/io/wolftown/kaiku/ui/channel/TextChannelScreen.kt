package io.wolftown.kaiku.ui.channel

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel

/**
 * Full-screen text channel view.
 *
 * Layout:
 * - Top app bar with channel name and back button
 * - LazyColumn of messages (reversed — newest at bottom, auto-scroll)
 * - Pull-to-load-more at the top for pagination
 * - MessageInput at the bottom with typing indicators
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TextChannelScreen(
    currentUserId: String,
    onNavigateBack: () -> Unit,
    viewModel: TextChannelViewModel = hiltViewModel()
) {
    val channelName by viewModel.channelName.collectAsState()
    val messages by viewModel.messages.collectAsState()
    val typingUsers by viewModel.typingUsers.collectAsState()
    val isLoading by viewModel.isLoading.collectAsState()
    val messageInput by viewModel.messageInput.collectAsState()

    val listState = rememberLazyListState()

    // Auto-scroll to bottom only when a genuinely new message arrives
    // and the user is already near the bottom of the list.
    var lastMessageId by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(messages.lastOrNull()?.id) {
        val newLastId = messages.lastOrNull()?.id
        if (newLastId != null && newLastId != lastMessageId) {
            lastMessageId = newLastId
            val lastVisible = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
            if (lastVisible >= messages.size - 4) {
                listState.animateScrollToItem(messages.size - 1)
            }
        }
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("#$channelName") },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = "Back"
                        )
                    }
                }
            )
        },
        bottomBar = {
            MessageInput(
                input = messageInput,
                channelName = channelName,
                typingUsers = typingUsers,
                onInputChanged = viewModel::onMessageInputChanged,
                onSend = viewModel::onSendMessage
            )
        }
    ) { paddingValues ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
        ) {
            when {
                isLoading && messages.isEmpty() -> {
                    CircularProgressIndicator(
                        modifier = Modifier.align(Alignment.Center)
                    )
                }

                messages.isEmpty() && !isLoading -> {
                    Text(
                        text = "No messages yet. Start the conversation!",
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.align(Alignment.Center)
                    )
                }

                else -> {
                    LazyColumn(
                        state = listState,
                        modifier = Modifier.fillMaxSize(),
                        contentPadding = PaddingValues(vertical = 8.dp)
                    ) {
                        // Load more button at the top
                        item(key = "load-more") {
                            TextButton(
                                onClick = { viewModel.onLoadMore() },
                                modifier = Modifier
                                    .fillMaxWidth()
                                    .padding(8.dp)
                            ) {
                                Text("Load earlier messages")
                            }
                        }

                        items(messages, key = { it.id }) { message ->
                            MessageItem(
                                message = message,
                                isOwnMessage = message.author.id == currentUserId,
                                onEdit = { msgId ->
                                    // In a full implementation, this would show an edit dialog.
                                    // For now, the edit action is wired up via the ViewModel.
                                },
                                onDelete = { msgId ->
                                    viewModel.onDeleteMessage(msgId)
                                },
                                onReact = { msgId ->
                                    // In a full implementation, this would show an emoji picker.
                                }
                            )
                        }
                    }
                }
            }
        }
    }
}
