package io.wolftown.kaiku.ui.security

import io.wolftown.kaiku.domain.model.CursorPage
import io.wolftown.kaiku.domain.model.SecurityEvent
import io.wolftown.kaiku.testutil.FakeSecurityApi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test

@OptIn(ExperimentalCoroutinesApi::class)
class SecurityLogViewModelTest {
    @Before fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())
    @After fun tearDown() = Dispatchers.resetMain()

    private fun ev(id: String) = SecurityEvent(id = id, eventType = "sign_in", createdAt = "2026-11-03T12:00:00Z")

    @Test fun `security log pages with show older`() = runTest {
        val api = FakeSecurityApi()
        api.pages.addLast(CursorPage(listOf(ev("e3"), ev("e2")), hasMore = true, nextCursor = "e2"))
        api.pages.addLast(CursorPage(listOf(ev("e1")), hasMore = false))
        val v = SecurityLogViewModel(api)
        v.load()
        assertEquals(listOf("e3", "e2"), v.ui.value.items.map { it.id })
        assertTrue(v.ui.value.hasMore)
        v.showOlder()
        assertEquals(listOf("events:", "events:e2"), api.calls)
        assertEquals(listOf("e3", "e2", "e1"), v.ui.value.items.map { it.id })
        assertFalse(v.ui.value.hasMore)
    }

    @Test fun `a recreated screen keeps the pages already shown`() = runTest {
        val api = FakeSecurityApi()
        api.pages.addLast(CursorPage(listOf(ev("e3"), ev("e2")), hasMore = true, nextCursor = "e2"))
        api.pages.addLast(CursorPage(listOf(ev("e1")), hasMore = false))
        val v = SecurityLogViewModel(api)
        v.loadOnce()
        v.showOlder()
        v.loadOnce() // rotation: the screen's effect runs again
        assertEquals(listOf("e3", "e2", "e1"), v.ui.value.items.map { it.id })
        assertEquals(2, api.calls.count { it.startsWith("events") })
    }
}
