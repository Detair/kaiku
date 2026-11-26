package io.wolftown.kaiku.approval

import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.LifecycleRegistry
import io.mockk.every
import io.mockk.mockk
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.async
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import java.security.Signature
import java.security.SignatureException

/**
 * The user-verification prompt is never waited for when it cannot be shown or its screen is gone,
 * and a key in the alias that is not the expected one is "changed", never "invalidated".
 */
@OptIn(ExperimentalCoroutinesApi::class)
class PromptWaitTest {

    private class Screen : LifecycleOwner {
        val registry: LifecycleRegistry = LifecycleRegistry.createUnsafe(this)
        override val lifecycle: Lifecycle get() = registry
        init {
            registry.currentState = Lifecycle.State.RESUMED
        }
    }

    @Test fun `an activity that has saved its state gets no prompt and is answered at once`() = runTest {
        val screen = Screen()
        var shown = false
        val r = awaitPrompt(screen.lifecycle, blocked = { true }, show = { shown = true }, withdraw = {})
        assertEquals(UvResult.Cancelled, r)
        assertFalse(shown)
    }

    @Test fun `an activity that is already gone gets no prompt either`() = runTest {
        val screen = Screen()
        screen.registry.currentState = Lifecycle.State.DESTROYED
        var shown = false
        assertEquals(UvResult.Cancelled, awaitPrompt(screen.lifecycle, blocked = { false }, show = { shown = true }, withdraw = {}))
        assertFalse(shown)
    }

    @Test fun `the wait ends when the activity the prompt was shown for is destroyed`() = runTest(UnconfinedTestDispatcher()) {
        val screen = Screen()
        var withdrawn = 0
        var shown = false
        val wait = async { awaitPrompt(screen.lifecycle, blocked = { false }, show = { shown = true }, withdraw = { withdrawn++ }) }
        assertTrue(shown)
        assertFalse(wait.isCompleted)
        // Rotated, or closed while in the background: the prompt went with it.
        screen.registry.handleLifecycleEvent(Lifecycle.Event.ON_DESTROY)
        assertEquals(UvResult.Cancelled, wait.await())
        assertEquals(1, withdrawn)
        assertEquals(0, screen.registry.observerCount)
    }

    @Test fun `a prompt that answers is answered`() = runTest(UnconfinedTestDispatcher()) {
        val screen = Screen()
        val signed = UvResult.Signed(byteArrayOf(1))
        val r = awaitPrompt(screen.lifecycle, blocked = { false }, show = { done -> done(signed); done(UvResult.Cancelled) }, withdraw = { fail() })
        assertEquals(signed, r)
        assertEquals(0, screen.registry.observerCount)
    }

    @Test fun `a per-use signature that fails on an invalidated key says so`() {
        val invalidated: android.security.keystore.KeyPermanentlyInvalidatedException = mockk {
            every { cause } returns null
        }
        val failing: Signature = mockk {
            every { update(any<ByteArray>()) } throws SignatureException("x").apply { initCause(invalidated) }
        }
        assertEquals(UvResult.KeyInvalidated, finishPerUse(failing, byteArrayOf(1)))
        val other: Signature = mockk {
            every { update(any<ByteArray>()) } throws SignatureException("keystore busy")
        }
        assertEquals(UvResult.Failed("SIGN_FAILED"), finishPerUse(other, byteArrayOf(1)))
        assertEquals(UvResult.Failed("UV_NO_CRYPTO_OBJECT"), finishPerUse(null, byteArrayOf(1)))
    }

    @Test fun `another key in the alias is changed, whatever state it is in`() {
        // The other key is invalidated: looking it up would throw. It is never looked up.
        var looked = false
        val lookup = { looked = true; throw KeyInvalidatedException() }
        try {
            keyFor<Any>("mine", hasAlias = { true }, publicId = { "another" }, privateKey = lookup)
            fail()
        } catch (_: KeyChangedException) {
        }
        assertFalse(looked)
        try {
            keyFor<Any>("mine", hasAlias = { false }, publicId = { null }, privateKey = lookup)
            fail()
        } catch (_: NoApprovalKeyException) {
        }
        assertFalse(looked)
        // This very key, invalidated: that is what is said.
        try {
            keyFor<Any>("mine", hasAlias = { true }, publicId = { "mine" }, privateKey = lookup)
            fail()
        } catch (_: KeyInvalidatedException) {
        }
        assertEquals("k", keyFor("mine", hasAlias = { true }, publicId = { "mine" }, privateKey = { "k" }))
    }
}
