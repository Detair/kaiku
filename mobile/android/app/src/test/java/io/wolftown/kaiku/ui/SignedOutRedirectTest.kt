package io.wolftown.kaiku.ui

import io.wolftown.kaiku.data.keydevice.AppEntry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The navigation of a signed-out phone goes by its entry, at every route. */
class SignedOutRedirectTest {

    private val routes = listOf(
        "server_url", "login", "register", "change_password", SCAN_SETUP_ROUTE, CONFIRM_ROUTE,
        SETUP_FLOW_ROUTE, SETUP_PENDING_ROUTE, "home", "settings", SECURITY_SETTINGS_ROUTE,
    )

    @Test fun `a stored setup that has not finished allows only its own routes`() {
        for (route in routes) {
            val expected = if (route in setOf(SCAN_SETUP_ROUTE, SETUP_FLOW_ROUTE, SETUP_PENDING_ROUTE)) null else SETUP_PENDING_ROUTE
            assertEquals(route, expected, signedOutRedirect(AppEntry.WAITING_SETUP, route))
        }
    }

    @Test fun `a lapsed key device never shows a sign-in without its key`() {
        for (route in routes) {
            val expected = if (route in setOf("server_url", "login", "register", "change_password")) CONFIRM_ROUTE else null
            assertEquals(route, expected, signedOutRedirect(AppEntry.CONFIRM_ITS_YOU, route))
        }
    }

    @Test fun `a phone without a key or setup goes anywhere it is led`() {
        for (entry in listOf(AppEntry.SIGN_IN, AppEntry.SERVER_ADDRESS, AppEntry.HOME, AppEntry.UNREADABLE)) {
            for (route in routes) assertNull("$entry $route", signedOutRedirect(entry, route))
        }
        assertNull(signedOutRedirect(AppEntry.WAITING_SETUP, null))
    }

    @Test fun `the start opens the route of the entry`() {
        assertEquals("home", entryRoute(AppEntry.HOME))
        assertEquals(CONFIRM_ROUTE, entryRoute(AppEntry.CONFIRM_ITS_YOU))
        assertEquals(SETUP_PENDING_ROUTE, entryRoute(AppEntry.WAITING_SETUP))
        assertEquals("login", entryRoute(AppEntry.SIGN_IN))
        assertEquals("server_url", entryRoute(AppEntry.SERVER_ADDRESS))
        assertNull(entryRoute(AppEntry.UNREADABLE))
    }

    /** The start and the navigation both go by the one function; neither looks at the phase itself. */
    @Test fun `the start and the navigation ask the one entry`() {
        val root = generateSequence(java.io.File("").absoluteFile) { it.parentFile }
            .map { java.io.File(it, "src/main/java/io/wolftown/kaiku") }
            .first { it.isDirectory }
        val activity = java.io.File(root, "ui/MainActivity.kt").readText()
        val start = activity.substringAfter("private fun resolveStartDestination()").substringBefore("\n    }")
        assertEquals(true, start.contains("keyDevices.entry(") && start.contains("entryRoute(entry)"))
        val nav = java.io.File(root, "ui/KaikuNavGraph.kt").readText()
        for (src in listOf(start, nav)) {
            assertEquals(false, src.contains("KeyDevicePhase.") || src.contains("KeyDeviceState."))
        }
        assertEquals(true, nav.contains("signedOutRedirect(entry, currentRoute)"))
    }
}
