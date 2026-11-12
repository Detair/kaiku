package io.wolftown.kaiku.ui.channel

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.heading
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import androidx.compose.foundation.gestures.scrollBy
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
import androidx.lifecycle.compose.LifecycleResumeEffect
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.typingNames
import io.wolftown.kaiku.ui.shared.ConnectionBanner
import io.wolftown.kaiku.ui.shared.NotificationsOffBanner
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
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
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
    val isKaikuDm by viewModel.isKaikuDm.collectAsState()
    val pendingMessages by viewModel.pendingMessages.collectAsState()
    val noticeActions: io.wolftown.kaiku.ui.security.NoticeActionsViewModel = androidx.hilt.navigation.compose.hiltViewModel()

    // Resumed = the user is looking at this chat: suppress and clear its notifications.
    LifecycleResumeEffect(viewModel) {
        viewModel.onVisible()
        onPauseOrDispose { viewModel.onHidden() }
    }

    val snackbarHostState = remember { SnackbarHostState() }
    val context = LocalContext.current
    // Not keyed on the error value: clearing it would re-key the effect and cancel the
    // snackbar. Clearing first lets a repeated identical error re-emit; collectLatest
    // replaces a showing snackbar with the newest error.
    LaunchedEffect(Unit) {
        viewModel.error.filterNotNull().collectLatest { res ->
            viewModel.clearError()
            val retry = res == R.string.chat_send_failed || res == R.string.chat_upload_failed
            // A "not sent" snackbar goes away once its message is deleted, retried or sent.
            val failedId = if (isPendingFailureRes(res)) viewModel.failureNotice.value else null
            val result = coroutineScope {
                val watcher = failedId?.let { id ->
                    launch {
                        viewModel.failureNotice.first { it != id }
                        snackbarHostState.currentSnackbarData?.dismiss()
                    }
                }
                snackbarHostState.showSnackbar(
                    message = context.getString(res),
                    actionLabel = if (retry) context.getString(R.string.common_retry) else null,
                    withDismissAction = true
                ).also { watcher?.cancel() }
            }
            if (retry && result == SnackbarResult.ActionPerformed) viewModel.onRetryFailed()
        }
    }

    val listState = rememberLazyListState()
    val zone = remember { ZoneId.systemDefault() }
    val rows = remember(messages, zone) { buildTimeline(messages, zone) }
    // List layout: "load-more" (index 0), the rows, then the pending messages.
    val lastIndex = rows.size + pendingMessages.size

    // AND-B-2: the keyboard must shrink the chat, not push the window (and the title bar) up.
    KeyboardResizesWindow()

    // While the user is at the end of the chat, keep the whole last item in view when it
    // grows (a failed message gets Retry and Delete) or the list gets smaller (keyboard, snackbar).
    val endFollower = remember { EndFollower() }
    // Set while older messages load; the list then keeps its place instead.
    var loadAnchorId by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(listState) {
        snapshotFlow { listState.isScrollInProgress to listState.canScrollForward }
            .collect { (scrolling, canScrollForward) ->
                endFollower.onScrollState(scrolling, canScrollForward)
                if (loadAnchorId == null && endFollower.shouldReturnToEnd(scrolling, canScrollForward)) {
                    listState.scrollToEnd()
                }
            }
    }

    // When the keyboard opens while the newest message is in view, keep it in view.
    val imeVisible = WindowInsets.isImeVisible
    LaunchedEffect(imeVisible) {
        if (!imeVisible) return@LaunchedEffect
        val lastVisible = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: return@LaunchedEffect
        if (lastVisible >= lastIndex - 1) listState.scrollToItem(lastIndex)
    }

    // When set, the emoji picker is shown for this message id.
    var reactionTargetId by remember { mutableStateOf<String?>(null) }
    // AND-B-20: when set, "Delete this message?" is asked for this id.
    var deleteTargetId by remember { mutableStateOf<String?>(null) }

    val atBottom by remember {
        derivedStateOf {
            val info = listState.layoutInfo
            val lastVisible = info.visibleItemsInfo.lastOrNull()?.index ?: 0
            info.totalItemsCount == 0 || lastVisible >= info.totalItemsCount - 2
        }
    }
    // AND-B-21: messages from others that arrived while scrolled up.
    var unseenCount by remember { mutableIntStateOf(0) }
    LaunchedEffect(atBottom) { if (atBottom) unseenCount = 0 }

    // AND-B-11: a short, polite TalkBack announcement for new messages of others.
    val announcer = remember { LiveAnnouncer() }
    LaunchedEffect(announcer) { announcer.run() }
    val announcement by announcer.text.collectAsState()

    // Jump to the newest message on the first non-empty load; afterwards
    // auto-scroll only when a genuinely new message arrives and the user is
    // already near the bottom of the list. Loading older messages does not
    // change the last id, so it never scrolls.
    var lastMessageId by remember { mutableStateOf<String?>(null) }
    var initialScrollDone by remember { mutableStateOf(false) }

    LaunchedEffect(messages.lastOrNull()?.id) {
        val newLastId = messages.lastOrNull()?.id ?: return@LaunchedEffect
        if (newLastId == lastMessageId) return@LaunchedEffect
        val incoming = newIncoming(lastMessageId, messages, currentUserId)
        lastMessageId = newLastId

        if (incoming.isNotEmpty()) {
            announcer.announce(
                if (incoming.size == 1) {
                    context.getString(R.string.chat_new_message_from, incoming[0].author.displayName)
                } else {
                    context.resources.getQuantityString(R.plurals.chat_new_messages, incoming.size, incoming.size)
                }
            )
        }
        if (!initialScrollDone) {
            initialScrollDone = true
            listState.scrollToItem(lastIndex)
        } else {
            val lastVisible = listState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0
            if (lastVisible >= rows.size - 4) {
                listState.animateScrollToItem(lastIndex)
            } else {
                unseenCount += incoming.size
            }
        }
    }

    // A message the user just sent appears at the bottom as "Sending…": show it.
    var lastPendingCount by remember { mutableIntStateOf(pendingMessages.size) }
    LaunchedEffect(pendingMessages.size) {
        val grew = pendingMessages.size > lastPendingCount
        lastPendingCount = pendingMessages.size
        if (grew) listState.animateScrollToItem(lastIndex)
    }

    // AND-B-21: reaching the top loads older messages; the view stays on the
    // message that was first, instead of jumping to the oldest loaded one.
    LaunchedEffect(listState, canLoadMore) {
        snapshotFlow { listState.firstVisibleItemIndex == 0 && initialScrollDone }
            .collect { atTop ->
                if (atTop && canLoadMore && loadAnchorId == null) {
                    val anchor = messages.firstOrNull()?.id
                    if (viewModel.onLoadMore()) loadAnchorId = anchor
                }
            }
    }
    val isLoadingMore by viewModel.isLoadingMore.collectAsState()
    LaunchedEffect(isLoadingMore) {
        if (isLoadingMore) return@LaunchedEffect
        val anchor = loadAnchorId ?: return@LaunchedEffect
        val index = rows.indexOfFirst { it is TimelineRow.Item && it.message.id == anchor }
        if (index >= 0 && messages.firstOrNull()?.id != anchor) {
            // +1 for "load-more"; -1 to show the day separator above it when there is one.
            val target = (index + 1 - if (index > 0 && rows[index - 1] is TimelineRow.Day) 1 else 0)
            listState.scrollToItem(target)
            endFollower.leftEnd()
        }
        loadAnchorId = null
    }

    // AND-B-4: open, share and save attachments.
    val attachmentViewModel: AttachmentViewModel = hiltViewModel()
    val attachmentBusy by attachmentViewModel.busy.collectAsState()
    var viewerAttachment by remember { mutableStateOf<io.wolftown.kaiku.domain.model.Attachment?>(null) }
    // The photo viewer is a dialog above this screen: its messages ("Saved to your phone.") go there.
    val viewerSnackbarHostState = remember { SnackbarHostState() }
    var saveAsMime by remember { mutableStateOf("application/octet-stream") }
    val saveAsLauncher = rememberLauncherForActivityResult(
        object : ActivityResultContracts.CreateDocument("*/*") {
            override fun createIntent(context: android.content.Context, input: String): android.content.Intent =
                super.createIntent(context, input).setType(saveAsMime)
        }
    ) { uri -> attachmentViewModel.saveTo(uri) }
    LaunchedEffect(attachmentViewModel) {
        attachmentViewModel.events.collect { event ->
            when (event) {
                is AttachmentEvent.Open -> startAttachmentIntent(
                    context,
                    android.content.Intent(android.content.Intent.ACTION_VIEW)
                        .setDataAndType(event.uri, event.mimeType),
                )
                is AttachmentEvent.Share -> startAttachmentIntent(
                    context,
                    android.content.Intent.createChooser(
                        android.content.Intent(android.content.Intent.ACTION_SEND)
                            .setType(event.mimeType)
                            .putExtra(android.content.Intent.EXTRA_STREAM, event.uri)
                            .addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION),
                        null,
                    ),
                )
                is AttachmentEvent.ChooseSaveTarget -> {
                    saveAsMime = event.mimeType
                    saveAsLauncher.launch(event.fileName)
                }
                is AttachmentEvent.Message -> {
                    val host = if (viewerAttachment != null) viewerSnackbarHostState else snackbarHostState
                    // Not awaited: the next event must not wait until this message is gone.
                    launch {
                        host.showSnackbar(message = context.getString(event.text), withDismissAction = true)
                    }
                }
            }
        }
    }
    val openAttachment: (io.wolftown.kaiku.domain.model.Attachment) -> Unit = { att ->
        if (att.isImage()) viewerAttachment = att else attachmentViewModel.open(att)
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
            stringResource(R.string.chat_composer_placeholder, t.text)
        }
    }

    Scaffold(
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
            // C28: only Kaiku writes in the Kaiku DM.
            if (isKaikuDm) {
                Text(
                    text = stringResource(R.string.security_kaiku_dm_read_only),
                    style = MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.fillMaxWidth().navigationBarsPadding().padding(16.dp),
                )
            } else MessageInput(
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
        if (!isKaikuDm) NotificationsOffBanner()
        Box(modifier = Modifier.weight(1f).fillMaxWidth()) {
            when {
                isLoading && messages.isEmpty() -> {
                    CircularProgressIndicator(
                        modifier = Modifier.align(Alignment.Center)
                    )
                }

                messages.isEmpty() && pendingMessages.isEmpty() && !isLoading -> {
                    Text(
                        text = stringResource(R.string.chat_empty),
                        style = MaterialTheme.typography.bodyLarge,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.align(Alignment.Center)
                    )
                }

                else -> {
                    val messagesById = remember(messages) { messages.associateBy { it.id } }
                    val today = remember(messages) { java.time.LocalDate.now(zone) }
                    LazyColumn(
                        state = listState,
                        modifier = Modifier.fillMaxSize(),
                        contentPadding = PaddingValues(vertical = 8.dp)
                    ) {
                        // Index 0 is always this item (empty when there is
                        // no older history) so row indices stay stable.
                        item(key = "load-more") {
                            if (canLoadMore) {
                                TextButton(
                                    onClick = {
                                        val anchor = messages.firstOrNull()?.id
                                        if (viewModel.onLoadMore()) loadAnchorId = anchor
                                    },
                                    modifier = Modifier
                                        .fillMaxWidth()
                                        .padding(8.dp)
                                ) {
                                    Text(stringResource(R.string.chat_load_earlier))
                                }
                            }
                        }

                        items(rows, key = { it.key }) { row ->
                            val message = when (row) {
                                is TimelineRow.Day -> {
                                    DaySeparator(dayLabel(row.date, today))
                                    return@items
                                }
                                is TimelineRow.Item -> row.message
                            }
                            val notice = message.securityNotice
                            // C28: notices exist only in the Kaiku DM, from the system account; a
                            // "security" message anywhere else is shown as an ordinary message.
                            if (isSecurityNotice(isKaikuDm, message) && notice != null) {
                                // No long-press menu, no reactions on notices.
                                io.wolftown.kaiku.ui.security.SecurityNoticeCard(notice, noticeActions, ownUserId = currentUserId)
                                return@items
                            }
                            MessageItem(
                                message = message,
                                isOwnMessage = message.author.id == currentUserId,
                                currentUserId = currentUserId,
                                onToggleReaction = { msgId, emoji, mine ->
                                    if (mine) viewModel.onRemoveReaction(msgId, emoji)
                                    else viewModel.onAddReaction(msgId, emoji)
                                },
                                onEdit = viewModel::onStartEdit,
                                onDelete = { msgId -> deleteTargetId = msgId },
                                onReact = { msgId ->
                                    reactionTargetId = msgId
                                },
                                repliedTo = message.replyTo?.let { messagesById[it] },
                                onOpenAttachment = openAttachment,
                            )
                        }
                        // AND-B-3: own messages the server has not confirmed yet, or that failed.
                        items(pendingMessages, key = { "pending-" + it.localId }) { pending ->
                            PendingMessageItem(
                                item = pending,
                                onRetry = viewModel::onRetryPending,
                                onDiscard = viewModel::onDiscardPending,
                            )
                        }
                    }

                    // AND-B-21: back to the newest message, with the number of new ones.
                    if (!atBottom) {
                        val jumpLabel = if (unseenCount > 0) {
                            pluralStringResource(R.plurals.chat_new_messages, unseenCount, unseenCount)
                        } else {
                            stringResource(R.string.chat_jump_to_newest)
                        }
                        val scope = rememberCoroutineScope()
                        SmallFloatingActionButton(
                            onClick = { scope.launch { listState.animateScrollToItem(lastIndex) } },
                            containerColor = MaterialTheme.colorScheme.primaryContainer,
                            contentColor = MaterialTheme.colorScheme.onPrimaryContainer,
                            modifier = Modifier
                                .align(Alignment.BottomEnd)
                                .padding(16.dp)
                                .semantics { contentDescription = jumpLabel },
                        ) {
                            Row(
                                verticalAlignment = Alignment.CenterVertically,
                                modifier = Modifier.padding(horizontal = 8.dp),
                            ) {
                                if (unseenCount > 0) {
                                    Text(unseenCount.toString(), style = MaterialTheme.typography.labelLarge)
                                }
                                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = null)
                            }
                        }
                    }
                }
            }
            // AND-B-11: polite live region; TalkBack reads it after what it is reading now.
            Box(
                modifier = Modifier
                    .size(1.dp)
                    .clearAndSetSemantics {
                        if (announcement.isNotEmpty()) contentDescription = announcement
                        liveRegion = LiveRegionMode.Polite
                    }
            )
        }
        // In the layout below the list, not floating over it: a snackbar must never
        // cover the end of the chat (a failed message's Retry and Delete).
        SnackbarHost(snackbarHostState)
        }
    }

    viewerAttachment?.let { att ->
        ImageViewer(
            attachment = att,
            busy = attachmentBusy,
            snackbarHostState = viewerSnackbarHostState,
            onSave = { attachmentViewModel.save(att) },
            onShare = { attachmentViewModel.share(att) },
            onDismiss = {
                viewerAttachment = null
                viewerSnackbarHostState.currentSnackbarData?.dismiss()
            },
        )
    }

    deleteTargetId?.let { messageId ->
        AlertDialog(
            onDismissRequest = { deleteTargetId = null },
            title = { Text(stringResource(R.string.chat_delete_confirm_title)) },
            text = { Text(stringResource(R.string.chat_delete_confirm_text)) },
            confirmButton = {
                TextButton(onClick = {
                    deleteTargetId = null
                    viewModel.onDeleteMessage(messageId)
                }) {
                    Text(stringResource(R.string.chat_delete), color = MaterialTheme.colorScheme.error)
                }
            },
            dismissButton = {
                TextButton(onClick = { deleteTargetId = null }) {
                    Text(stringResource(R.string.common_cancel))
                }
            },
        )
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

/** A Kaiku notice card: a `security` message from the system account in the Kaiku DM only. */
internal fun isSecurityNotice(isKaikuDm: Boolean, message: io.wolftown.kaiku.domain.model.Message): Boolean =
    isKaikuDm && message.author.isSystem && message.messageType == "security"


/** A centered day label between messages of different days (AND-B-21). */
@Composable
private fun DaySeparator(label: DayLabel) {
    val text = when (label) {
        DayLabel.Today -> stringResource(R.string.chat_day_today)
        DayLabel.Yesterday -> stringResource(R.string.chat_day_yesterday)
        is DayLabel.Date -> remember(label.date) {
            label.date.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM))
        }
    }
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .padding(vertical = 8.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.semantics { heading() },
        )
    }
}

/** Scrolls to the very end, also when the last item is taller than the view. */
private suspend fun androidx.compose.foundation.lazy.LazyListState.scrollToEnd() {
    val last = layoutInfo.totalItemsCount - 1
    if (last < 0) return
    scrollToItem(last)
    var steps = 0
    while (canScrollForward && steps++ < 8) {
        scrollBy(layoutInfo.viewportSize.height.coerceAtLeast(1).toFloat())
    }
}

/** Starts an intent that hands an attachment to another app, with read access. */
private fun startAttachmentIntent(context: android.content.Context, intent: android.content.Intent) {
    intent.addFlags(android.content.Intent.FLAG_GRANT_READ_URI_PERMISSION or android.content.Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
        context.startActivity(intent)
    } catch (_: android.content.ActivityNotFoundException) {
        android.widget.Toast.makeText(context, R.string.chat_attachment_no_app, android.widget.Toast.LENGTH_LONG).show()
    }
}
