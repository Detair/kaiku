package io.wolftown.kaiku.di

import android.content.Context
import dagger.Binds
import dagger.Module
import dagger.Provides
import dagger.hilt.InstallIn
import dagger.hilt.android.qualifiers.ApplicationContext
import dagger.hilt.components.SingletonComponent
import dagger.multibindings.IntoSet
import io.wolftown.kaiku.push.AndroidNotificationPresenter
import io.wolftown.kaiku.push.AndroidPushPlatform
import io.wolftown.kaiku.push.FcmTransport
import io.wolftown.kaiku.push.NotificationLedger
import io.wolftown.kaiku.push.NotificationSink
import io.wolftown.kaiku.push.PrefsNotificationLedger
import io.wolftown.kaiku.push.PrefsPushRegistrationStore
import io.wolftown.kaiku.push.PushPlatform
import io.wolftown.kaiku.push.PushRegistrationScheduler
import io.wolftown.kaiku.push.PushRegistrationStore
import io.wolftown.kaiku.push.PushTransport
import io.wolftown.kaiku.push.WakeWorkEnqueuer
import io.wolftown.kaiku.push.WorkManagerPushRegistrationScheduler
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

    @Binds
    abstract fun bindPushPlatform(impl: AndroidPushPlatform): PushPlatform

    @Binds
    abstract fun bindPushRegistrationScheduler(
        impl: WorkManagerPushRegistrationScheduler,
    ): PushRegistrationScheduler

    /** Push transports the registrar chooses from (UnifiedPush joins in Task 8). */
    @Binds
    @IntoSet
    abstract fun bindFcmTransport(impl: FcmTransport): PushTransport

    companion object {
        /** Plain prefs: the ledger holds message ids and timestamps only, never content. */
        @Provides
        @Singleton
        fun provideNotificationLedger(
            @ApplicationContext context: Context,
        ): NotificationLedger = PrefsNotificationLedger(
            context.getSharedPreferences("notif_ledger", Context.MODE_PRIVATE),
        )

        /** Plain prefs: provider, account id and an endpoint hash, never the endpoint. */
        @Provides
        @Singleton
        fun providePushRegistrationStore(
            @ApplicationContext context: Context,
        ): PushRegistrationStore = PrefsPushRegistrationStore(
            context.getSharedPreferences("push_prefs", Context.MODE_PRIVATE),
        )
    }
}
