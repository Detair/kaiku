package io.wolftown.kaiku.ui.home

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import coil3.compose.AsyncImage
import io.wolftown.kaiku.domain.model.Guild

@Composable
fun GuildSidebar(
    guilds: List<Guild>,
    selectedGuildId: String?,
    onGuildSelected: (String) -> Unit,
    modifier: Modifier = Modifier
) {
    LazyColumn(
        modifier = modifier
            .fillMaxHeight()
            .width(72.dp)
            .background(MaterialTheme.colorScheme.surfaceContainerLow),
        horizontalAlignment = Alignment.CenterHorizontally,
        contentPadding = PaddingValues(vertical = 8.dp)
    ) {
        items(guilds, key = { it.id }) { guild ->
            GuildIcon(
                guild = guild,
                isSelected = guild.id == selectedGuildId,
                onClick = { onGuildSelected(guild.id) }
            )
            Spacer(modifier = Modifier.height(4.dp))
        }
    }
}

@Composable
private fun GuildIcon(
    guild: Guild,
    isSelected: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier
) {
    val backgroundColor = if (isSelected) {
        MaterialTheme.colorScheme.primary
    } else {
        MaterialTheme.colorScheme.surfaceContainerHigh
    }

    val textColor = if (isSelected) {
        MaterialTheme.colorScheme.onPrimary
    } else {
        MaterialTheme.colorScheme.onSurface
    }

    Box(
        modifier = modifier
            .size(48.dp)
            .clip(CircleShape)
            .background(backgroundColor)
            .semantics { role = Role.Button }
            .clickable(
                onClick = onClick,
                onClickLabel = "Select ${guild.name}"
            ),
        contentAlignment = Alignment.Center
    ) {
        if (guild.iconUrl != null) {
            AsyncImage(
                model = guild.iconUrl,
                contentDescription = guild.name,
                modifier = Modifier.fillMaxSize(),
                contentScale = ContentScale.Crop
            )
        } else {
            Text(
                text = guild.name.take(1).uppercase(),
                color = textColor,
                fontSize = 20.sp,
                fontWeight = FontWeight.Bold,
                textAlign = TextAlign.Center
            )
        }
    }
}
