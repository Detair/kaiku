package io.wolftown.kaiku.data.call

import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.repository.DmRepository
import javax.inject.Inject

/** Names from the cached DM list (loaded on every connect). */
class DmCallDirectory @Inject constructor(
    private val dms: DmRepository,
    private val authState: AuthState,
) : CallDirectory {
    override fun nameOf(channelId: String, userId: String): String? =
        dms.conversations.value.firstOrNull { it.id == channelId }
            ?.participants?.firstOrNull { it.userId == userId }
            ?.let { it.displayName.ifBlank { it.username } }
            ?.takeIf { it.isNotBlank() }

    override fun titleOf(channelId: String): String? {
        val me = authState.signedInUserId() ?: return null
        val dm = dms.conversations.value.firstOrNull { it.id == channelId } ?: return null
        return DmRepository.conversationTitle(dm, me).takeIf { it.isNotBlank() }
    }
}
