package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Category
import io.wolftown.kaiku.domain.model.Channel
import io.wolftown.kaiku.domain.model.ChannelType
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ChannelSectionsTest {

    private fun ch(id: String, position: Int, categoryId: String? = null) = Channel(
        id = id,
        name = id,
        channelType = ChannelType.TEXT,
        categoryId = categoryId,
        position = position,
    )

    @Test
    fun `sections follow category position with uncategorized first`() {
        val categories = listOf(
            Category(id = "c-late", name = "Late", position = 5),
            Category(id = "c-early", name = "Early", position = 1),
        )
        val channels = listOf(
            ch("a", 0, "c-late"),
            ch("b", 1, "c-early"),
            ch("loose", 2),
            ch("c", 0, "c-early"),
        )

        val sections = groupChannels(channels, categories)

        assertEquals(listOf<String?>(null, "Early", "Late"), sections.map { it.title })
        assertEquals(listOf("loose"), sections[0].channels.map { it.id })
        // Channels keep position order inside a section.
        assertEquals(listOf("c", "b"), sections[1].channels.map { it.id })
        assertEquals(listOf("a"), sections[2].channels.map { it.id })
    }

    @Test
    fun `unknown category id yields headerless section`() {
        val sections = groupChannels(
            channels = listOf(ch("a", 0, "gone-uuid")),
            categories = listOf(Category(id = "c1", name = "Known")),
        )

        assertEquals(1, sections.size)
        assertNull(sections.single().title)
        assertEquals(listOf("a"), sections.single().channels.map { it.id })
    }

    @Test
    fun `categories without channels produce no section`() {
        val sections = groupChannels(
            channels = listOf(ch("a", 0)),
            categories = listOf(Category(id = "c1", name = "Empty")),
        )

        assertEquals(1, sections.size)
        assertNull(sections.single().title)
    }
}
