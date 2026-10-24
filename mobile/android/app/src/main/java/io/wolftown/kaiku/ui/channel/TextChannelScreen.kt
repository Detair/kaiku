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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.typingNames
import io.wolftown.kaiku.ui.shared.ConnectionBanner
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.filterNotNull

/**
 * Full-screen text channel view.
 *
 * Layout:
 * - Top app bar with channel name and back button
 * - LazyColumn of messages, oldest at the top and newest at the bottom
 *   (opens scrolled to the newest message, auto-scrolls on new ones)
 * - "Load earlier messages" at the top while the server reports more history
 * - MessageInput at the bottom with typing indicators
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TextChannelScreen(
    currentUserId: String,
    onNavigateBack: () -> Unit,
    viewModel: TextChannelViewModel = hiltViewModel()
) {
    val title by viewModel.title.collectAsState()
    val messages by viewModel.messages.collectAsState()
    val isUploading by viewModel.isUploading.collectAsState()
    val typingUserIds by viewModel.typingUserIds.collectAsState()
    val memberNames by viewModel.memberNames.collectAsState()
    val someone = stringResource(R.string.chat_typing_someone)
    val typingLabels = remember(typingUserIds, memberNames, someone) {
        typingNames(typingUserIds, memberNames, someone)
    }
    val isLoading by viewModel.isLoading.collectAsState()
    val canLoadMore by viewModel.canLoadMore.collectAsState()
    val messageInput by viewModel.messageInput.collectAsState()
    val editingMessage by viewModel.editingMessage.collectAsState()
    val connectionStatus by viewModel.connectionStatus.collectAsState()

    val snackbarHostState = remember { SnackbarHostState() }
    val context = LocalContext.current
    // Not keyed on the error value: clearing it would re-key the effect and cancel the
    // snackbar. Clearing first lets a repeated identical error re-emit; collectLatest
    // replaces a showing snackbar with the newest error.
    LaunchedEffect(Unit) {
        viewModel.error.filterNotNull().collectLatest { res ->
            viewModel.clearError()
            val retry = res == R.string.chat_send_failed
            val result = snackbarHostState.showSnackbar(
                message = context.getString(res),
                actionLabel = if (retry) context.getString(R.string.common_retry) else null,
                withDismissAction = true
            )
            if (retry && result == SnackbarResult.ActionPerformed) viewModel.onSendMessage()
        }
    }

    val listState = rememberLazyListState()

    // When set, the emoji picker is shown for this message id.
    var reactionTargetId by remember { mutableStateOf<String?>(null) }

    // Jump to the newest message on the first non-empty load; afterwards
    // auto-scroll only when a genuinely new message arrives and the user is
    // already near the bottom of the list. Loading older messages does not
    // change the last id, so it never scrolls.
    var lastMessageId by remember { mutableStateOf<String?>(null) }
    var initialScrollDone by remember { mutableStateOf(false) }

    LaunchedEffect(messages.lastOrNull()?.id) {
        val newLastId = messages.lastOrNull()?.id ?: return@LaunchedEffect
        if (newLastId == lastMessageId) return@LaunchedEffect
        lastMessageId = newLastId

        // The "load-more" item always sits at index 0, so the last message
        // is at index messages.size.
        val lastIndex = messages.size
        if (!initialScrollDone) {
            initialScrollDone = true
            listState.scrollToItem(lastIndex)
        } else {
            val lastVisible = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
            if (lastVisible >= lastIndex - 4) {
                listState.animateScrollToItem(lastIndex)
            }
        }
    }

    val titleText = when (val t = title) {
        null -> stringResource(R.string.chat_title_loading)
        else -> if (t.isDm) t.text else stringResource(R.string.chat_title_channel, t.text)
    }
    val placeholder = when (val t = title) {
        null -> stringResource(R.string.chat_message_placeholder_loading)
        else -> if (t.isDm) {
            stringResource(R.string.chat_message_placeholder_dm, t.text)
        } else {
            stringResource(R.string.chat_message_placeholder_channel, t.text)
        }
    }

    Scaffold(
        snackbarHost = { SnackbarHost(snackbarHostState) },
        topBar = {
            TopAppBar(
                title = { Text(titleText) },
                navigationIcon = {
                    IconButton(onClick = onNavigateBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Filled.ArrowBack,
                            contentDescription = stringResource(R.string.common_back)
                        )
                    }
                }
            )
        },
        bottomBar = {
            MessageInput(
                input = messageInput,
                placeholder = placeholder,
                typingNames = typingLabels,
                onInputChanged = viewModel::onMessageInputChanged,
                onSend = viewModel::onSendMessage,
                isUploading = isUploading,
                onAttachmentPicked = viewModel::onAttachmentPicked
            )
        }
    ) { paddingValues ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(paddingValues)
        ) {
        ConnectionBanner(status = connectionStatus)
        Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
            when {
                isLoading && messages.isEmpty() -> {
                    CircularProgressIndicator(
                        modifier = Modifier.align(Alignment.Center)
                    )
                }

                messages.isEmpty() && !isLoading -> {
                    Text(
                        text = stringResource(R.string.chat_empty),
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
                        // Index 0 is always this item (empty when there is
                        // no older history) so message indices stay stable.
                        item(key = "load-more") {
                            if (canLoadMore) {
                                TextButton(
                                    onClick = { viewModel.onLoadMore() },
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(8.dp)
                                ) {
                                    Text(stringResource(R.string.chat_load_earlier))
                                }
                            }
                        }

                        items(messages, key = { it.id }) { message ->
                            MessageItem(
                                message = message,
                                isOwnMessage = message.author.id == currentUserId,
                                currentUserId = currentUserId,
                                onToggleReaction = { msgId, emoji, mine ->
                                    if (mine) viewModel.onRemoveReaction(msgId, emoji)
                                    else viewModel.onAddReaction(msgId, emoji)
                                },
                                onEdit = viewModel::onStartEdit,
                                onDelete = { msgId ->
                                    viewModel.onDeleteMessage(msgId)
                                },
                                onReact = { msgId ->
                                    reactionTargetId = msgId
                                }
                            )
                        }
                    }
                }
            }
        }
        }
    }

    editingMessage?.let { message ->
        EditMessageDialog(
            initial = message.content,
            onConfirm = viewModel::onConfirmEdit,
            onDismiss = viewModel::onCancelEdit
        )
    }

    reactionTargetId?.let { messageId ->
        EmojiPicker(
            onEmojiSelected = { emoji ->
                viewModel.onAddReaction(messageId, emoji)
                reactionTargetId = null
            },
            onDismiss = { reactionTargetId = null }
        )
    }
}
