package io.wolftown.kaiku.data.keydevice

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

/**
 * The complete list of places that can forget the record, delete the key, or write the record,
 * read from the sources: a new caller fails here until it is added on purpose.
 */
class KeyForgetCallSitesTest {

    private val root = generateSequence(File("").absoluteFile) { it.parentFile }
        .map { File(it, "src/main/java/io/wolftown/kaiku") }
        .first { it.isDirectory }

    private fun filesWith(pattern: Regex): Set<String> = root.walkTopDown()
        .filter { it.isFile && it.extension == "kt" }
        .filter { f -> f.readLines().any { line -> !line.trim().startsWith("*") && !line.trim().startsWith("//") && pattern.containsMatchIn(line) } }
        .map { it.relativeTo(root).path }
        .toSet()

    @Test fun `a record is forgotten only for a server answer or an invalidated key`() {
        // Who calls it (it is defined in the repository).
        assertEquals(setOf("data/keydevice/KeyAnswers.kt", "data/keydevice/KeyInvalidation.kt"), filesWith(Regex("""\.forget\(""")))
        assertEquals(setOf("data/keydevice/KeyDeviceRepository.kt"), filesWith(Regex("""fun forget\(""")))
        assertEquals(setOf("data/keydevice/KeyAnswers.kt"), filesWith(Regex("""Forget\.SERVER_ANSWER""")))
        assertEquals(setOf("data/keydevice/KeyInvalidation.kt"), filesWith(Regex("""Forget\.KEY_INVALIDATED""")))
    }

    @Test fun `answers settle keys in these places only`() {
        assertEquals(
            setOf(
                "data/keydevice/KeyAnswers.kt",
                "data/auth/DeviceSignIn.kt",
                "data/auth/KeyDeviceSignOut.kt",
                "data/auth/PhoneRemoval.kt",
                "ui/auth/ConfirmItsYouViewModel.kt",
                "ui/auth/setup/SetupOwner.kt",
            ),
            filesWith(Regex("""\.settle\(|fun settle\(""")),
        )
    }

    @Test fun `the Keystore alias is touched only by the repository`() {
        assertEquals(setOf("data/keydevice/KeyDeviceRepository.kt"), filesWith(Regex("""[kK]eyStore\.(delete|generate)\(""")))
        assertEquals(setOf("approval/ApprovalKeyStore.kt"), filesWith(Regex("""kaiku_approve_v1""")))
        // A signature is asked from the alias in the wrapper's own verifier only, always for a named key.
        assertEquals(setOf("approval/UserVerifier.kt"), filesWith(Regex("""keyStore\.signature\(""")))
        assertTrue(filesWith(Regex("""\.signature\(\)""")).isEmpty())
        assertEquals(
            setOf("data/keydevice/KeyDeviceRepository.kt", "ui/auth/setup/SetupOwner.kt"),
            filesWith(Regex("""\.deleteUnclaimedKey\(|fun deleteUnclaimedKey\(|\.generateKey\(|fun generateKey\(""")),
        )
        // The store itself: the repository, the module that makes it, and the clean-up of the flag an
        // earlier version left in it (which never touches the record).
        assertEquals(
            setOf(
                "data/keydevice/KeyDeviceStore.kt", "data/keydevice/KeyDeviceRepository.kt", "di/ApprovalModule.kt",
                "data/auth/LegacyRemovalSession.kt",
            ),
            filesWith(Regex("""\bKeyDeviceStore\b""")),
        )
    }

    @Test fun `the record is written only through the compare-and-set operations`() {
        assertEquals(
            setOf(
                "data/keydevice/KeyDeviceRepository.kt",
                "data/keydevice/KeyAnswers.kt",
                "data/auth/DeviceSignIn.kt",
                "data/auth/KeyDeviceSignOut.kt",
                "ui/auth/setup/SetupOwner.kt",
                "ui/MainActivity.kt",
            ),
            filesWith(Regex("""keyDevices\.(create|replace|update|markRenewalPrompted)\(""")) + "data/keydevice/KeyDeviceRepository.kt",
        )
    }

    @Test fun `the removal of a signed-out phone has no way to the stored session`() {
        val text = File(root, "data/auth/PhoneRemoval.kt").readText()
        for (shared in listOf("TokenStorage", "AuthState", "AuthRepository", "RealtimeConnectionManager", "PushRegistrar", "TokenRefresher", "KaikuHttpClient")) {
            assertTrue("PhoneRemoval must not know $shared", !Regex("""\b$shared\b""").containsMatchIn(text.lines().filter { !it.trim().startsWith("*") }.joinToString("\n")))
        }
    }
}
