package io.wolftown.kaiku.data.local

import android.content.SharedPreferences
import java.nio.ByteBuffer
import java.security.GeneralSecurityException
import java.util.Base64

/**
 * [SharedPreferences] whose values are encrypted by [cipher] before they reach
 * the plain [backing] file: each value is stored as base64(blob) with the key
 * name as AAD, so a value copied under another name fails to decrypt. Key
 * names stay readable (they are not secret). A failed decrypt throws
 * [GeneralSecurityException], which [TokenStorage] turns into a reset.
 * Strings, longs, ints, booleans and floats; string sets are not supported.
 */
class KeystoreWrappedPreferences(
    private val backing: SharedPreferences,
    private val cipher: ValueCipher,
) : SharedPreferences {

    private companion object {
        const val T_STRING: Byte = 1
        const val T_LONG: Byte = 2
        const val T_INT: Byte = 3
        const val T_BOOLEAN: Byte = 4
        const val T_FLOAT: Byte = 5
    }

    private fun encode(key: String, type: Byte, payload: ByteArray): String =
        Base64.getEncoder().encodeToString(cipher.encrypt(byteArrayOf(type) + payload, key.toByteArray(Charsets.UTF_8)))

    private fun decode(key: String): Pair<Byte, ByteArray>? {
        val stored = backing.getString(key, null) ?: return null
        val blob = try {
            Base64.getDecoder().decode(stored)
        } catch (_: IllegalArgumentException) {
            throw GeneralSecurityException("stored value is not base64")
        }
        val plain = cipher.decrypt(blob, key.toByteArray(Charsets.UTF_8))
        if (plain.isEmpty()) throw GeneralSecurityException("empty value")
        return plain[0] to plain.copyOfRange(1, plain.size)
    }

    private fun value(type: Byte, payload: ByteArray): Any = when (type) {
        T_STRING -> String(payload, Charsets.UTF_8)
        T_LONG -> ByteBuffer.wrap(payload).long
        T_INT -> ByteBuffer.wrap(payload).int
        T_BOOLEAN -> payload.isNotEmpty() && payload[0] != 0.toByte()
        T_FLOAT -> ByteBuffer.wrap(payload).float
        else -> throw GeneralSecurityException("unknown value type")
    }

    private inline fun <reified T> typed(key: String?, def: T): T {
        if (key == null) return def
        val (type, payload) = decode(key) ?: return def
        return value(type, payload) as? T ?: throw ClassCastException("stored value of $key has another type")
    }

    override fun getAll(): MutableMap<String, *> =
        backing.all.keys.associateWith { k -> decode(k)?.let { (t, p) -> value(t, p) } }.toMutableMap()

    override fun getString(key: String?, defValue: String?): String? = typed(key, defValue)
    override fun getLong(key: String?, defValue: Long): Long = typed(key, defValue)
    override fun getInt(key: String?, defValue: Int): Int = typed(key, defValue)
    override fun getBoolean(key: String?, defValue: Boolean): Boolean = typed(key, defValue)
    override fun getFloat(key: String?, defValue: Float): Float = typed(key, defValue)

    override fun getStringSet(key: String?, defValues: MutableSet<String>?): MutableSet<String>? =
        throw UnsupportedOperationException("string sets are not stored encrypted")

    override fun contains(key: String?): Boolean = backing.contains(key)

    override fun edit(): SharedPreferences.Editor = Editor(backing.edit())

    override fun registerOnSharedPreferenceChangeListener(listener: SharedPreferences.OnSharedPreferenceChangeListener?) =
        backing.registerOnSharedPreferenceChangeListener(listener)

    override fun unregisterOnSharedPreferenceChangeListener(listener: SharedPreferences.OnSharedPreferenceChangeListener?) =
        backing.unregisterOnSharedPreferenceChangeListener(listener)

    private inner class Editor(private val inner: SharedPreferences.Editor) : SharedPreferences.Editor {
        private fun put(key: String?, type: Byte, payload: ByteArray) = apply {
            requireNotNull(key) { "key" }
            inner.putString(key, encode(key, type, payload))
        }

        override fun putString(key: String?, value: String?) =
            if (value == null) remove(key) else put(key, T_STRING, value.toByteArray(Charsets.UTF_8))

        override fun putLong(key: String?, value: Long) = put(key, T_LONG, ByteBuffer.allocate(8).putLong(value).array())
        override fun putInt(key: String?, value: Int) = put(key, T_INT, ByteBuffer.allocate(4).putInt(value).array())
        override fun putBoolean(key: String?, value: Boolean) = put(key, T_BOOLEAN, byteArrayOf(if (value) 1 else 0))
        override fun putFloat(key: String?, value: Float) = put(key, T_FLOAT, ByteBuffer.allocate(4).putFloat(value).array())

        override fun putStringSet(key: String?, values: MutableSet<String>?): SharedPreferences.Editor =
            throw UnsupportedOperationException("string sets are not stored encrypted")

        override fun remove(key: String?) = apply { inner.remove(key) }
        override fun clear() = apply { inner.clear() }
        override fun commit(): Boolean = inner.commit()
        override fun apply() = inner.apply()
    }
}
