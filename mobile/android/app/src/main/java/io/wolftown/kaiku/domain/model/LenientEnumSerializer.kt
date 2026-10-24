package io.wolftown.kaiku.domain.model

import kotlinx.serialization.KSerializer
import kotlinx.serialization.SerializationException
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

/**
 * Enum serializer that tolerates values this client does not know (C5).
 *
 * Decoding maps any wire value that matches no entry to [unknown] instead of
 * failing, so a newer server adding an enum value cannot blank a whole list.
 * Entries are matched by [wireName], the string the server uses.
 *
 * Encoding writes the wire name. [unknown] is never sent: it only exists as a
 * decode result, so encoding it throws [SerializationException].
 */
abstract class LenientEnumSerializer<T : Enum<T>>(
    serialName: String,
    private val values: Array<T>,
    private val unknown: T,
    private val wireName: (T) -> String,
) : KSerializer<T> {

    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor(serialName, PrimitiveKind.STRING)

    override fun deserialize(decoder: Decoder): T {
        val raw = decoder.decodeString()
        return values.firstOrNull { it != unknown && wireName(it) == raw } ?: unknown
    }

    override fun serialize(encoder: Encoder, value: T) {
        if (value == unknown) {
            throw SerializationException("${descriptor.serialName}.UNKNOWN must never be serialized")
        }
        encoder.encodeString(wireName(value))
    }
}
