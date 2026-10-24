package io.wolftown.kaiku.push

import android.content.Context
import android.content.SharedPreferences
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton

/**
 * The user's push transport choices (Settings): UnifiedPush instead of FCM
 * and which distributor app to use. Lives in `push_prefs` next to the
 * registration record under different keys; neither store clears the other's.
 * Holds no endpoints.
 */
@Singleton
class PushPreferences @Inject constructor(
    @ApplicationContext context: Context,
) {
    private val prefs: SharedPreferences =
        context.getSharedPreferences("push_prefs", Context.MODE_PRIVATE)

    var preferUnifiedPush: Boolean
        get() = prefs.getBoolean(KEY_PREFER_UP, false)
        set(value) {
            prefs.edit().putBoolean(KEY_PREFER_UP, value).apply()
        }

    /** Package name of the chosen UnifiedPush distributor, or null for "first installed". */
    var distributor: String?
        get() = prefs.getString(KEY_DISTRIBUTOR, null)
        set(value) {
            prefs.edit().apply {
                if (value == null) remove(KEY_DISTRIBUTOR) else putString(KEY_DISTRIBUTOR, value)
            }.apply()
        }

    private companion object {
        const val KEY_PREFER_UP = "pref_unifiedpush"
        const val KEY_DISTRIBUTOR = "pref_distributor"
    }
}
