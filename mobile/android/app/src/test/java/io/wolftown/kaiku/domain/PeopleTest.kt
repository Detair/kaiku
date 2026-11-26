package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.GuildMember
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.Locale

class PeopleTest {

    private fun m(id: String, user: String, display: String = "", nick: String? = null) =
        GuildMember(userId = id, username = user, displayName = display, nickname = nick)

    @Test
    fun `members of several servers appear once, without me and without blocked people`() {
        val family = listOf(m("me", "fabian", "Fabian"), m("a", "anna", "Anna"), m("o", "oma", "Oma"))
        val friends = listOf(m("a", "anna", "Anna"), m("b", "bert", "Bert"), m("x", "troll", "Troll"))

        val people = messageCandidates(listOf(family, friends), me = "me", blocked = setOf("x"), locale = Locale.GERMAN)

        assertEquals(listOf("a", "b", "o"), people.map { it.userId })
    }

    @Test
    fun `the account's display name is shown and a server nickname is kept for search`() {
        val people = messageCandidates(listOf(listOf(m("o", "oma", "Gertrud", nick = "Oma"))), me = null, blocked = emptySet())

        val gertrud = people.single()
        assertEquals("Gertrud", gertrud.name)
        assertTrue(gertrud.matches("oma"))
        assertTrue(gertrud.matches("gert"))
    }

    @Test
    fun `without a display name the login name is shown`() {
        val p = messageCandidates(listOf(listOf(m("o", "oma"))), me = null, blocked = emptySet()).single()
        assertEquals("oma", p.name)
        assertNull(p.nickname)
    }

    @Test
    fun `sorting follows the language, ignoring case and accents`() {
        val people = messageCandidates(
            listOf(listOf(m("1", "z", "zoe"), m("2", "o", "Özlem"), m("3", "a", "anna"), m("4", "b", "Bernd"))),
            me = null, blocked = emptySet(), locale = Locale.GERMAN,
        )
        assertEquals(listOf("anna", "Bernd", "Özlem", "zoe"), people.map { it.name })
    }

    @Test
    fun `search matches display name and login name, case and accent insensitive`() {
        val p = Person(userId = "1", name = "Jürgen Müller", username = "jmueller", avatarUrl = null)
        assertTrue(p.matches("MULLER"))
        assertTrue(p.matches("jürg"))
        assertTrue(p.matches("jmue"))
        assertTrue(p.matches("  "))
        assertFalse(p.matches("anna"))
    }
}
