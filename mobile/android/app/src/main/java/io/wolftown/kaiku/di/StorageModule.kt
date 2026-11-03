package io.wolftown.kaiku.di

import android.content.Context
import android.content.SharedPreferences
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKey
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.data.local.KeystoreValueCipher
import io.wolftown.kaiku.data.local.KeystoreWrappedPreferences
import io.wolftown.kaiku.data.local.LegacyTokenMigration
import io.wolftown.kaiku.data.local.TokenStorage
import java.io.File
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
object StorageModule {

    /** Plain file holding only Keystore-encrypted values (`MODE_PRIVATE`; backups are off). */
    private const val SESSION_FILE = "kaiku_session_v2"

    /** The old `security-crypto` store, read once for the move and then deleted. */
    private const val LEGACY_FILE = "kaiku_secure_prefs"

    private const val APP_STATE_FILE = "kaiku_app_state"

    /**
     * The session store: values encrypted under the AndroidKeyStore key
     * `kaiku_storage_v1`. On first open after the upgrade, an existing
     * `kaiku_secure_prefs` is opened once with `security-crypto`, its session
     * copied over, and deleted; a damaged one is dropped and the person signs in
     * again (as before). `security-crypto` stays one release for this read only
     * (coordinator call 4) and is removed in the release after v1.0.
     */
    @Provides
    @Singleton
    fun provideSessionPreferences(
        @ApplicationContext context: Context
    ): SharedPreferences {
        val prefs = KeystoreWrappedPreferences(
            context.getSharedPreferences(SESSION_FILE, Context.MODE_PRIVATE),
            KeystoreValueCipher(context),
        )
        val migrated = LegacyTokenMigration.run(
            legacyExists = File(context.applicationInfo.dataDir, "shared_prefs/$LEGACY_FILE.xml").exists(),
            openLegacy = { openLegacy(context) },
            deleteLegacy = { context.deleteSharedPreferences(LEGACY_FILE) },
            target = prefs,
            keys = TokenStorage.STORED_KEYS,
        )
        if (!migrated) {
            // Flag for the UI to say "Please sign in again" on next launch.
            context.getSharedPreferences(APP_STATE_FILE, Context.MODE_PRIVATE)
                .edit()
                .putBoolean(TokenStorage.KEY_RESET_NOTICE, true)
                .apply()
        }
        return prefs
    }

    private fun openLegacy(context: Context): SharedPreferences {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()
        return EncryptedSharedPreferences.create(
            context,
            LEGACY_FILE,
            masterKey,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
        )
    }

    @Provides
    @Singleton
    fun provideTokenStorage(
        prefs: SharedPreferences,
        @ApplicationContext context: Context
    ): TokenStorage {
        // The app-state flag is also set above when the old store was lost at the move.
        return TokenStorage(prefs, context.getSharedPreferences(APP_STATE_FILE, Context.MODE_PRIVATE))
    }
}
