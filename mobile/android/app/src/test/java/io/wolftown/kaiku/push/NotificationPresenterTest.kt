package io.wolftown.kaiku.push

import android.app.Notification
import android.app.NotificationManager
import android.content.Context
import android.service.notification.StatusBarNotification
import io.mockk.every
import io.mockk.mockk
import io.mockk.verify
import io.mockk.verifyOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

/** Presenter bookkeeping against a mocked NotificationManager (notifications themselves are not built). */
class NotificationPresenterTest {

    private val ch = "0f8fad5b-d9cb-469f-a165-70867728950e"
    private val callA = "6f1c2a3b-4d5e-4f60-8a7b-9c0d1e2f3a4b"
    private val callB = "1b2c3d4e-5f60-4a7b-8c9d-0e1f2a3b4c5d"

    private val manager: NotificationManager = mockk(relaxed = true)
    private val context: Context = mockk()
    private val plans = mutableListOf<Pair<CallNotification, CallPostPlan>>()
    private val presenter = AndroidNotificationPresenter(
        context,
        callBuilder = { n, plan ->
            plans += n to plan
            mockk<Notification>(relaxed = true)
        },
    )

    @Before
    fun setUp() {
        every { context.getSystemService(NotificationManager::class.java) } returns manager
        every { manager.areNotificationsEnabled() } returns true
        every { manager.activeNotifications } returns emptyArray()
    }

    private fun call(kind: CallKind, callId: String = callA) = CallNotification("me", ch, callId, kind, "Bob")

    @Test
    fun `reading the channel removes only the message notification, never the call`() {
        presenter.cancel(ch)

        verify { manager.cancel(ch, AndroidNotificationPresenter.MESSAGE_ID) }
        verify(exactly = 0) { manager.cancel(ch, AndroidNotificationPresenter.CALL_ID) }
    }

    @Test
    fun `cancelCall removes only the call notification`() {
        presenter.cancelCall(ch)

        verify { manager.cancel(ch, AndroidNotificationPresenter.CALL_ID) }
        verify(exactly = 0) { manager.cancel(ch, AndroidNotificationPresenter.MESSAGE_ID) }
    }

    @Test
    fun `missed call cancels the ringing post before posting`() {
        presenter.showCall { call(CallKind.INCOMING) }
        presenter.showCall { call(CallKind.MISSED) }

        verifyOrder {
            manager.notify(ch, AndroidNotificationPresenter.CALL_ID, any())
            manager.cancel(ch, AndroidNotificationPresenter.CALL_ID)
            manager.notify(ch, AndroidNotificationPresenter.CALL_ID, any())
        }
        val missed = plans.last().second
        assertNull(missed.timeoutMs)
        assertTrue(missed.silent)
    }

    @Test
    fun `new ringing call after a missed one is posted fresh and alerts`() {
        presenter.showCall { call(CallKind.MISSED, callA) }
        presenter.showCall { call(CallKind.INCOMING, callB) }

        verify(exactly = 2) { manager.cancel(ch, AndroidNotificationPresenter.CALL_ID) }
        val ringing = plans.last().second
        assertTrue(ringing.cancelFirst)
        assertFalse(ringing.onlyAlertOnce)
        assertEquals(CallNotificationText.RINGING_TIMEOUT_MS, ringing.timeoutMs)
    }

    @Test
    fun `repeat of the same ringing call is updated in place`() {
        presenter.showCall { call(CallKind.INCOMING) }
        presenter.showCall { call(CallKind.INCOMING) }

        verify(exactly = 1) { manager.cancel(ch, AndroidNotificationPresenter.CALL_ID) }
        verify(exactly = 2) { manager.notify(ch, AndroidNotificationPresenter.CALL_ID, any()) }
        assertTrue(plans.last().second.onlyAlertOnce)
    }

    @Test
    fun `call notifications are posted under their own id`() {
        presenter.showCall { call(CallKind.INCOMING) }

        verify { manager.notify(ch, AndroidNotificationPresenter.CALL_ID, any()) }
        verify(exactly = 0) { manager.notify(ch, AndroidNotificationPresenter.MESSAGE_ID, any()) }
    }

    @Test
    fun `cancelAll also removes call notifications`() {
        val active: StatusBarNotification = mockk {
            every { id } returns AndroidNotificationPresenter.CALL_ID
            every { tag } returns ch
        }
        every { manager.activeNotifications } returns arrayOf(active)

        presenter.cancelAll()

        verify { manager.cancel(ch, AndroidNotificationPresenter.CALL_ID) }
    }

    @Test
    fun `refused call posts nothing`() {
        assertEquals(ShowResult.REFUSED, presenter.showCall { null })
        verify(exactly = 0) { manager.notify(any<String>(), any(), any()) }
    }

    @Test
    fun `disabled notifications post nothing`() {
        every { manager.areNotificationsEnabled() } returns false
        assertEquals(ShowResult.DISABLED, presenter.showCall { call(CallKind.INCOMING) })
        verify(exactly = 0) { manager.notify(any<String>(), any(), any()) }
    }
}
