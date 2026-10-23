package io.wolftown.kaiku.domain

import io.wolftown.kaiku.domain.model.Message

/** What the message list shows as a message's body. */
sealed interface MessageBody {
    /** Plain message content, rendered as inline markdown. */
    data class Text(val markdown: String) : MessageBody

    /** End-to-end encrypted content this client cannot decrypt yet. */
    data object EncryptedPlaceholder : MessageBody
}

/**
 * Decides how [message] is shown. Encrypted messages never expose their raw
 * content (ciphertext); they show a placeholder instead.
 */
fun messageDisplayContent(message: Message): MessageBody =
    if (message.encrypted) MessageBody.EncryptedPlaceholder
    else MessageBody.Text(message.content)
