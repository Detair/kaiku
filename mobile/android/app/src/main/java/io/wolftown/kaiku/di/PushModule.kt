package io.wolftown.kaiku.di

import android.content.Context
import dagger.Binds
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import io.wolftown.kaiku.push.AndroidNotificationPresenter
import io.wolftown.kaiku.push.NotificationLedger
import io.wolftown.kaiku.push.NotificationSink
import io.wolftown.kaiku.push.PrefsNotificationLedger
import io.wolftown.kaiku.push.WakeWorkEnqueuer
import io.wolftown.kaiku.push.WorkManagerWakeEnqueuer
import javax.inject.Singleton

@Module
@InstallIn(SingletonComponent::class)
abstract class PushModule {

    @Binds
    @Singleton
    abstract fun bindNotificationSink(impl: AndroidNotificationPresenter): NotificationSink

    @Binds
    @Singleton
    abstract fun bindWakeWorkEnqueuer(impl: WorkManagerWakeEnqueuer): WakeWorkEnqueuer

    companion object {
        /** Plain prefs: the ledger holds message ids and timestamps only, never content. */
        @Provides
        @Singleton
        fun provideNotificationLedger(
            @ApplicationContext context: Context,
        ): NotificationLedger = PrefsNotificationLedger(
            context.getSharedPreferences("notif_ledger", Context.MODE_PRIVATE),
        )
    }
}
