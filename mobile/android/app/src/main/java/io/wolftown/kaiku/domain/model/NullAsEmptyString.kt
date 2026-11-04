package io.wolftown.kaiku.domain.model

import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.KSerializer
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder

/**
 * A display string that decodes JSON `null` as `""` (C5 tolerance).
 *
 * The field's own default covers a missing key, not an explicit `null`: an
 * authenticator whose row is gone (a pending passkey deleted with its
 * request) came back as `label: null` from servers before the C28 fix, and a
 * non-null `String` then failed the whole security-log page or Kaiku DM.
 */
@OptIn(ExperimentalSerializationApi::class)
object NullAsEmptyString : KSerializer<String> {
    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor("io.wolftown.kaiku.NullAsEmptyString", PrimitiveKind.STRING)

    override fun deserialize(decoder: Decoder): String =
        if (decoder.decodeNotNullMark()) decoder.decodeString() else decoder.decodeNull().let { "" }

    override fun serialize(encoder: Encoder, value: String) = encoder.encodeString(value)
}
