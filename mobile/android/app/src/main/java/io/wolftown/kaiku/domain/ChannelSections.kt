package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Category
import io.wolftown.kaiku.domain.model.Channel

/** A run of channels under one header; [title] null means "no header". */
data class ChannelSection(val title: String?, val channels: List<Channel>)

/**
 * Groups [channels] by category for the channel list.
 *
 * Uncategorised channels come first (no header), then one section per
 * category in category `position` order. A channel whose category id is not
 * in [categories] is shown headerless rather than under a raw id. Channels
 * keep `position` order inside a section.
 */
fun groupChannels(channels: List<Channel>, categories: List<Category>): List<ChannelSection> {
    val byId = categories.associateBy { it.id }
    val sorted = channels.sortedBy { it.position }

    val (headerless, categorised) = sorted.partition { it.categoryId == null || it.categoryId !in byId }
    val sections = mutableListOf<ChannelSection>()
    if (headerless.isNotEmpty()) sections += ChannelSection(title = null, channels = headerless)

    val grouped = categorised.groupBy { it.categoryId!! }
    categories.sortedBy { it.position }.forEach { category ->
        val members = grouped[category.id] ?: return@forEach
        sections += ChannelSection(title = category.name, channels = members)
    }
    return sections
}
