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
import io.wolftown.kaiku.data.local.TokenStorage
import java.util.logging.Level
import java.util.logging.Logger
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
object StorageModule {

    private val logger = Logger.getLogger("StorageModule")

    @Provides
    @Singleton
    fun provideEncryptedSharedPreferences(
        @ApplicationContext context: Context
    ): SharedPreferences {
        val masterKey = MasterKey.Builder(context)
            .setKeyScheme(MasterKey.KeyScheme.AES256_GCM)
            .build()

        return try {
            EncryptedSharedPreferences.create(
                context,
                "kaiku_secure_prefs",
                masterKey,
                EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
            )
        } catch (e: Exception) {
            // Encrypted prefs can be corrupted after OS upgrades or backup restores.
            // Delete the corrupted file and recreate.
            logger.log(Level.WARNING, "EncryptedSharedPreferences corrupted, recreating", e)
            context.deleteSharedPreferences("kaiku_secure_prefs")

            // Flag for the UI to show a notification on next launch
            context.getSharedPreferences("kaiku_app_state", Context.MODE_PRIVATE)
                .edit()
                .putBoolean(TokenStorage.KEY_RESET_NOTICE, true)
                .commit()

            EncryptedSharedPreferences.create(
                context,
                "kaiku_secure_prefs",
                masterKey,
                EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
                EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM
            )
        }
    }

    @Provides
    @Singleton
    fun provideTokenStorage(
        prefs: SharedPreferences,
        @ApplicationContext context: Context
    ): TokenStorage {
        // The app-state flag is also set above when the store was recreated at open.
        return TokenStorage(prefs, context.getSharedPreferences("kaiku_app_state", Context.MODE_PRIVATE))
    }
}
