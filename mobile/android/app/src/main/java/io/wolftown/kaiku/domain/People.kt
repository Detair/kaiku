package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.GuildMember
import java.text.Collator
import java.text.Normalizer
import java.util.Locale

/**
 * Someone the user can start a conversation with ("New message").
 * [name] is the account's display name (shown first); [nickname] is a
 * server nickname that differs from it (searchable, not shown); [username]
 * is the login name.
 */
data class Person(
    val userId: String,
    val name: String,
    val username: String,
    val avatarUrl: String?,
    val nickname: String? = null,
)

/**
 * Everyone on the user's servers, once: de-duplicated by user id (the first
 * server's nickname is kept for search), without the user themself and without people
 * in [blocked]. Sorted by shown name, as the people in [locale] sort.
 */
fun messageCandidates(
    membersByServer: List<List<GuildMember>>,
    me: String?,
    blocked: Set<String>,
    locale: Locale = Locale.getDefault(),
): List<Person> {
    val seen = HashSet<String>()
    me?.let(seen::add)
    seen.addAll(blocked)
    val people = ArrayList<Person>()
    for (members in membersByServer) {
        for (m in members) {
            if (!seen.add(m.userId)) continue
            val accountName = m.displayName.ifBlank { m.username }
            val nickname = m.nickname?.takeIf { it.isNotBlank() }
            people += Person(
                userId = m.userId,
                name = accountName,
                username = m.username,
                avatarUrl = m.avatarUrl,
                nickname = nickname?.takeIf { it != accountName },
            )
        }
    }
    val collator = Collator.getInstance(locale).apply { strength = Collator.PRIMARY }
    return people.sortedWith { a, b -> collator.compare(a.name, b.name).takeIf { it != 0 } ?: a.username.compareTo(b.username) }
}

/** Whether [person] matches the search [query]: display name, nickname or login name; case and accents ignored. */
fun Person.matches(query: String): Boolean {
    val q = searchKey(query.trim())
    if (q.isEmpty()) return true
    return searchKey(name).contains(q) || searchKey(username).contains(q) || (nickname?.let { searchKey(it).contains(q) } ?: false)
}

private val marks = Regex("\\p{Mn}+")

private fun searchKey(text: String): String =
    marks.replace(Normalizer.normalize(text, Normalizer.Form.NFD), "").lowercase(Locale.ROOT)
