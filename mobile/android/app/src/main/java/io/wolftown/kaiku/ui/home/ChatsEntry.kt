package io.wolftown.kaiku.ui.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Email
import androidx.compose.material3.Badge
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.ChatsSummary
import io.wolftown.kaiku.ui.dm.conversationPreview

/**
 * The first thing on the start page: "Chats" with a conversation (the newest
 * unread one, else the newest) and the number of unread direct messages; one
 * tap opens the chat list. Shown whether or not the user is on a server.
 */
@Composable
fun ChatsEntry(summary: ChatsSummary, currentUserId: String, onOpen: () -> Unit) {
    val newest = summary.newest
    val subtitle = if (newest == null) {
        stringResource(R.string.home_chats_empty)
    } else {
        val kaiku = newest.isSystem && newest.readOnly
        val title = if (kaiku) stringResource(R.string.security_kaiku_dm_name) else DmRepository.conversationTitle(newest, currentUserId)
        // Like the chat list: the Kaiku chat says "Security", never its raw text.
        val preview = if (kaiku) stringResource(R.string.security_kaiku_dm_subtitle) else conversationPreview(newest, currentUserId)
        // "Anna · You: ok": a separator, so the preview's own "Name:" is not doubled.
        stringResource(R.string.home_chats_preview, title, preview)
    }
    Row(
        Modifier
            .fillMaxWidth()
            .heightIn(min = 64.dp)
            .clickable(onClick = onOpen)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Default.Email, contentDescription = null, tint = MaterialTheme.colorScheme.onSurface, modifier = Modifier.size(28.dp))
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f)) {
            Text(
                stringResource(R.string.home_chats),
                style = MaterialTheme.typography.titleMedium,
                fontWeight = if (summary.unread > 0) FontWeight.SemiBold else FontWeight.Normal,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                subtitle,
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (summary.unread > 0) {
            Spacer(Modifier.width(8.dp))
            UnreadBadge(summary.unread)
        }
    }
    HorizontalDivider()
}

/** The unread count ("99+" above 99), read out as "3 unread messages". */
@Composable
fun UnreadBadge(count: Int, modifier: Modifier = Modifier) {
    val description = pluralStringResource(R.plurals.home_chats_unread, count, count)
    Badge(modifier.semantics { contentDescription = description }) {
        Text(
            if (count > 99) stringResource(R.string.dm_unread_overflow) else count.toString(),
            modifier = Modifier.clearAndSetSemantics { },
        )
    }
}
