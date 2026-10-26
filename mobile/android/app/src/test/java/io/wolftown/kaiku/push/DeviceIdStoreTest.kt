package io.wolftown.kaiku.push

import android.content.Context
import android.content.SharedPreferences
import io.mockk.every
import io.mockk.mockk
import io.mockk.slot
import io.mockk.verify
import org.junit.Assert.assertEquals
import org.junit.Test
import java.util.UUID

class DeviceIdStoreTest {

    private var stored: String? = null
    private val editor: SharedPreferences.Editor = mockk(relaxed = true)
    private val prefs: SharedPreferences = mockk {
        every { getString("push_device_id", null) } answers { stored }
        every { edit() } returns editor
    }
    private val context: Context = mockk {
        every { getSharedPreferences("push_prefs", Context.MODE_PRIVATE) } returns prefs
    }

    init {
        val value = slot<String>()
        every { editor.putString("push_device_id", capture(value)) } answers {
            stored = value.captured
            editor
        }
        every { editor.commit() } returns true
    }

    @Test
    fun `new id is a uuid persisted synchronously`() {
        val id = DeviceIdStore(context).id()

        assertEquals(id, UUID.fromString(id).toString())
        assertEquals(id, stored)
        verify(exactly = 1) { editor.commit() }
        verify(exactly = 0) { editor.apply() }
    }

    @Test
    fun `id is created once and kept`() {
        val first = DeviceIdStore(context).id()
        val again = DeviceIdStore(context).id() // e.g. after a process restart

        assertEquals(first, again)
        verify(exactly = 1) { editor.commit() }
    }

    @Test
    fun `stored id is reused`() {
        stored = "6f1c2b9e-3d4a-4e5f-8a7b-9c0d1e2f3a4b"

        assertEquals(stored, DeviceIdStore(context).id())
        verify(exactly = 0) { editor.commit() }
    }
}
