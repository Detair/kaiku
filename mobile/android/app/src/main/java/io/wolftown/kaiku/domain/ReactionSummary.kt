package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Message
import io.wolftown.kaiku.domain.model.Reaction

/** What a reaction chip shows: the emoji, how many reacted, and whether you did. */
data class ReactionSummary(val emoji: String, val count: Int, val mine: Boolean)

fun reactionSummaries(message: Message, currentUserId: String?): List<ReactionSummary> =
    message.reactions.orEmpty()
        .filter { it.count > 0 }
        .map { r ->
            ReactionSummary(
                emoji = r.emoji,
                count = r.count,
                mine = r.me || (currentUserId != null && currentUserId in r.users),
            )
        }

/**
 * [reactions] after [userId] added [emoji]. [isMe] says the user is the
 * signed-in one. Idempotent: a repeated event changes nothing.
 */
fun List<Reaction>?.withReactionAdded(emoji: String, userId: String, isMe: Boolean): List<Reaction> {
    val current = this.orEmpty()
    val existing = current.firstOrNull { it.emoji == emoji }
        ?: return current + Reaction(emoji, 1, isMe, listOf(userId))
    if (userId in existing.users) return current
    val updated = existing.copy(
        count = existing.count + 1,
        me = existing.me || isMe,
        users = existing.users + userId,
    )
    return current.map { if (it.emoji == emoji) updated else it }
}

/** [reactions] after [userId] removed [emoji]; drops the reaction when nobody is left. */
fun List<Reaction>?.withReactionRemoved(emoji: String, userId: String, isMe: Boolean): List<Reaction> {
    val current = this.orEmpty()
    val existing = current.firstOrNull { it.emoji == emoji } ?: return current
    if (userId !in existing.users && !(isMe && existing.me)) return current
    val updated = existing.copy(
        count = existing.count - 1,
        me = existing.me && !isMe,
        users = existing.users - userId,
    )
    return if (updated.count <= 0) current.filter { it.emoji != emoji }
    else current.map { if (it.emoji == emoji) updated else it }
}
