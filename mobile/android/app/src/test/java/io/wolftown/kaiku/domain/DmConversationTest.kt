package io.wolftown.kaiku.domain

import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.domain.model.DmConversation
import io.wolftown.kaiku.domain.model.DmParticipant
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DmConversationTest {

    private val me = "user-me"

    private fun participant(id: String, name: String, avatar: String? = null) = DmParticipant(
        userId = id,
        username = name.lowercase(),
        displayName = name,
        avatarUrl = avatar,
    )

    private fun dm(
        id: String = "dm1",
        name: String = "",
        icon: String? = null,
        unread: Int = 0,
        participants: List<DmParticipant>,
    ) = DmConversation(
        id = id,
        name = name,
        iconUrl = icon,
        unreadCount = unread,
        participants = participants,
    )

    @Test
    fun conversationTitle_oneOnOne_usesOtherParticipant() {
        val conv = dm(participants = listOf(participant(me, "Me"), participant("u2", "Alice")))
        assertEquals("Alice", DmRepository.conversationTitle(conv, me))
    }

    @Test
    fun conversationTitle_oneOnOne_ignoresChannelName() {
        // A 1:1 DM should always show the other person, even if a name leaked in.
        val conv = dm(
            name = "auto-generated",
            participants = listOf(participant(me, "Me"), participant("u2", "Alice")),
        )
        assertEquals("Alice", DmRepository.conversationTitle(conv, me))
    }

    @Test
    fun conversationTitle_groupWithName_usesName() {
        val conv = dm(
            name = "Squad",
            participants = listOf(
                participant(me, "Me"),
                participant("u2", "Alice"),
                participant("u3", "Bob"),
            ),
        )
        assertEquals("Squad", DmRepository.conversationTitle(conv, me))
    }

    @Test
    fun conversationTitle_groupWithoutName_joinsOtherNames() {
        val conv = dm(
            participants = listOf(
                participant(me, "Me"),
                participant("u2", "Alice"),
                participant("u3", "Bob"),
            ),
        )
        assertEquals("Alice, Bob", DmRepository.conversationTitle(conv, me))
    }

    @Test
    fun conversationTitle_selfOnly_fallsBack() {
        val conv = dm(participants = listOf(participant(me, "Me")))
        assertEquals("Direct Message", DmRepository.conversationTitle(conv, me))
    }

    @Test
    fun conversationAvatar_oneOnOne_usesOtherParticipantAvatar() {
        val conv = dm(
            icon = "/group/icon",
            participants = listOf(
                participant(me, "Me", "/me.png"),
                participant("u2", "Alice", "/alice.png"),
            ),
        )
        assertEquals("/alice.png", DmRepository.conversationAvatarUrl(conv, me))
    }

    @Test
    fun conversationAvatar_group_usesGroupIcon() {
        val conv = dm(
            icon = "/group/icon",
            participants = listOf(
                participant(me, "Me"),
                participant("u2", "Alice", "/alice.png"),
                participant("u3", "Bob", "/bob.png"),
            ),
        )
        assertEquals("/group/icon", DmRepository.conversationAvatarUrl(conv, me))
    }

    @Test
    fun conversationAvatar_groupWithoutIcon_isNull() {
        val conv = dm(
            participants = listOf(
                participant(me, "Me"),
                participant("u2", "Alice"),
                participant("u3", "Bob"),
            ),
        )
        assertNull(DmRepository.conversationAvatarUrl(conv, me))
    }

    @Test
    fun clearUnread_resetsOnlyTheTargetConversation() {
        val before = listOf(
            dm(id = "a", unread = 5, participants = listOf(participant(me, "Me"), participant("u2", "Alice"))),
            dm(id = "b", unread = 3, participants = listOf(participant(me, "Me"), participant("u3", "Bob"))),
        )
        val after = DmRepository.clearUnread(before, "a")
        assertEquals(0, after.first { it.id == "a" }.unreadCount)
        assertEquals(3, after.first { it.id == "b" }.unreadCount)
    }
}
