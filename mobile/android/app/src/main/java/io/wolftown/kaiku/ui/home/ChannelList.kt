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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.wolftown.kaiku.R
import io.wolftown.kaiku.domain.ChannelSection
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType

/**
 * Displays channels grouped by category.
 *
 * Sections arrive ready-made from the ViewModel: uncategorised channels first
 * (no header), then one section per category with its name as the header.
 */
@Composable
fun ChannelList(
    sections: List<ChannelSection>,
    onChannelSelected: (channelId: String, channelType: ChannelType) -> Unit,
    modifier: Modifier = Modifier
) {
    LazyColumn(
        modifier = modifier.fillMaxSize(),
        contentPadding = PaddingValues(vertical = 8.dp)
    ) {
        sections.forEachIndexed { index, section ->
            if (section.title != null) {
                item(key = "section-$index") {
                    CategoryHeader(title = section.title)
                }
            }
            items(section.channels, key = { it.id }) { channel ->
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
    title: String,
    modifier: Modifier = Modifier
) {
    Text(
        text = title.uppercase(),
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
            ChannelType.VOICE -> stringResource(R.string.channel_prefix_voice)
            else -> stringResource(R.string.channel_prefix_text)
        }

        Text(
            text = prefix,
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(end = 6.dp)
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
                Text(
                    text = if (channel.unreadCount > 99) {
                        stringResource(R.string.channel_unread_overflow)
                    } else {
                        channel.unreadCount.toString()
                    }
                )
            }
        }

        if (channel.channelType == ChannelType.VOICE && channel.userLimit != null && channel.userLimit > 0) {
            Text(
                text = stringResource(R.string.channel_user_limit, channel.userLimit),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant
            )
        }
    }
}
