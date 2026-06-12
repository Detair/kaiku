package io.wolftown.kaiku.ui.home

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Badge
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType

/**
 * Displays channels grouped by category.
 *
 * Channels with null categoryId appear at the top, followed by groups
 * keyed by categoryId. Within each group, channels are already sorted
 * by position (the ViewModel handles sorting).
 */
@Composable
fun ChannelList(
    channels: List<Channel>,
    onChannelSelected: (channelId: String, channelType: ChannelType) -> Unit,
    modifier: Modifier = Modifier
) {
    // Group channels: null categoryId first, then by categoryId
    val uncategorized = channels.filter { it.categoryId == null }
    val categorized = channels
        .filter { it.categoryId != null }
        .groupBy { it.categoryId!! }

    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(vertical = 8.dp)
    ) {
        // Uncategorized channels at the top
        if (uncategorized.isNotEmpty()) {
            items(uncategorized, key = { it.id }) { channel ->
                ChannelItem(
                    channel = channel,
                    onClick = { onChannelSelected(channel.id, channel.channelType) }
                )
            }
        }

        // Categorized channel groups
        categorized.forEach { (categoryId, categoryChannels) ->
            item(key = "category-$categoryId") {
                CategoryHeader(categoryId = categoryId)
            }
            items(categoryChannels, key = { it.id }) { channel ->
                ChannelItem(
                    channel = channel,
                    onClick = { onChannelSelected(channel.id, channel.channelType) }
                )
            }
        }
    }
}

@Composable
private fun CategoryHeader(
    categoryId: String,
    modifier: Modifier = Modifier
) {
    Text(
        text = categoryId.uppercase(),
        style = MaterialTheme.typography.labelSmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        fontWeight = FontWeight.SemiBold,
        modifier = modifier.padding(horizontal = 16.dp, vertical = 12.dp)
    )
}

@Composable
private fun ChannelItem(
    channel: Channel,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    Row(
        modifier = modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically
    ) {
        val prefix = when (channel.channelType) {
            ChannelType.VOICE -> "\uD83D\uDD0A "  // speaker emoji as icon fallback
            else -> "# "
        }

        Text(
            text = prefix,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant
        )

        val hasUnread = channel.unreadCount > 0
        Text(
            text = channel.name,
            style = MaterialTheme.typography.bodyLarge,
            // Unread channels read brighter and bolder, matching the desktop.
            fontWeight = if (hasUnread) FontWeight.SemiBold else FontWeight.Normal,
            color = if (hasUnread) {
                MaterialTheme.colorScheme.onSurface
            } else {
                MaterialTheme.colorScheme.onSurfaceVariant
            },
            modifier = Modifier.weight(1f)
        )

        if (hasUnread) {
            Badge {
                Text(text = if (channel.unreadCount > 99) "99+" else channel.unreadCount.toString())
            }
        }

        if (channel.channelType == ChannelType.VOICE && channel.userLimit != null && channel.userLimit > 0) {
            Text(
                text = "/${channel.userLimit}",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}
