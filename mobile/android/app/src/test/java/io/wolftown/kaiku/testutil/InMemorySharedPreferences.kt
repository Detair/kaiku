package io.wolftown.kaiku.testutil

import android.content.SharedPreferences

/**
 * Minimal in-memory [SharedPreferences] for JVM unit tests, so the real
 * [io.wolftown.kaiku.data.local.TokenStorage] logic can be exercised without
 * Android. Edits are applied atomically on commit/apply. Listeners are not supported.
 */
class InMemorySharedPreferences : SharedPreferences {
    private val values = mutableMapOf<String, Any?>()

    @Synchronized
    override fun getAll(): MutableMap<String, *> = HashMap(values)

    @Synchronized
    override fun getString(key: String?, defValue: String?): String? =
        v(key) as String? ?: defValue

    @Suppress("UNCHECKED_CAST")
    @Synchronized
    override fun getStringSet(key: String?, defValues: MutableSet<String>?): MutableSet<String>? =
        (v(key) as Set<String>?)?.toMutableSet() ?: defValues

    @Synchronized
    override fun getInt(key: String?, defValue: Int): Int = v(key) as Int? ?: defValue

    @Synchronized
    override fun getLong(key: String?, defValue: Long): Long = v(key) as Long? ?: defValue

    @Synchronized
    override fun getFloat(key: String?, defValue: Float): Float = v(key) as Float? ?: defValue

    @Synchronized
    override fun getBoolean(key: String?, defValue: Boolean): Boolean =
        v(key) as Boolean? ?: defValue

    @Synchronized
    override fun contains(key: String?): Boolean = key != null && values.containsKey(key)

    private fun v(key: String?): Any? = if (key == null) null else values[key]

    override fun edit(): SharedPreferences.Editor = Editor()

    override fun registerOnSharedPreferenceChangeListener(
        listener: SharedPreferences.OnSharedPreferenceChangeListener?
    ) = Unit

    override fun unregisterOnSharedPreferenceChangeListener(
        listener: SharedPreferences.OnSharedPreferenceChangeListener?
    ) = Unit

    private inner class Editor : SharedPreferences.Editor {
        private val puts = mutableMapOf<String, Any?>()
        private val removals = mutableSetOf<String>()
        private var clearAll = false

        override fun putString(key: String?, value: String?) = put(key, value)
        override fun putStringSet(key: String?, values: MutableSet<String>?) = put(key, values?.toSet())
        override fun putInt(key: String?, value: Int) = put(key, value)
        override fun putLong(key: String?, value: Long) = put(key, value)
        override fun putFloat(key: String?, value: Float) = put(key, value)
        override fun putBoolean(key: String?, value: Boolean) = put(key, value)

        override fun remove(key: String?): SharedPreferences.Editor {
            if (key != null) removals.add(key)
            return this
        }

        override fun clear(): SharedPreferences.Editor {
            clearAll = true
            return this
        }

        override fun commit(): Boolean {
            synchronized(this@InMemorySharedPreferences) {
                if (clearAll) values.clear()
                removals.forEach { values.remove(it) }
                puts.forEach { (k, v) -> if (v == null) values.remove(k) else values[k] = v }
            }
            return true
        }

        override fun apply() {
            commit()
        }

        private fun put(key: String?, value: Any?): SharedPreferences.Editor {
            if (key != null) {
                removals.remove(key)
                puts[key] = value
            }
            return this
        }
    }
}
