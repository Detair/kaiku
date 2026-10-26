package io.wolftown.kaiku.push

import org.junit.Assert.assertEquals
import org.junit.Test

class NotificationPermissionPolicyTest {
    @Test fun `enabled notifications are granted`() {
        assertEquals(
            NotificationPermissionDecision.GRANTED,
            NotificationPermissionPolicy.decide(33, true, false, false)
        )
    }

    @Test fun `first ask on android 13 shows rationale`() {
        assertEquals(
            NotificationPermissionDecision.ASK_WITH_RATIONALE,
            NotificationPermissionPolicy.decide(33, false, false, false)
        )
    }

    @Test fun `permanently denied opens settings`() {
        assertEquals(
            NotificationPermissionDecision.OPEN_SETTINGS,
            NotificationPermissionPolicy.decide(33, false, true, false)
        )
    }

    @Test fun `disabled on android 12 opens settings`() {
        assertEquals(
            NotificationPermissionDecision.OPEN_SETTINGS,
            NotificationPermissionPolicy.decide(31, false, false, false)
        )
    }
}
