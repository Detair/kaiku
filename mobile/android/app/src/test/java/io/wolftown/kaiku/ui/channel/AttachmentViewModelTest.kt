package io.wolftown.kaiku.ui.channel

import io.mockk.coEvery
import io.mockk.coVerify
import io.mockk.mockk
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.repository.AttachmentFiles
import io.wolftown.kaiku.domain.model.Attachment
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import java.io.File

@OptIn(ExperimentalCoroutinesApi::class)
class AttachmentViewModelTest {

    private val dispatcher = StandardTestDispatcher()
    private val files: AttachmentFiles = mockk(relaxed = true)
    private val photo = Attachment(
        id = "a1", filename = "photo.jpg", mimeType = "image/jpeg", size = 10, url = "/files/a1",
    )

    @Before
    fun setUp() = Dispatchers.setMain(dispatcher)

    @After
    fun tearDown() = Dispatchers.resetMain()

    @Test
    fun `a second tap on Save while saving starts no second save`() = runTest(dispatcher) {
        val gate = CompletableDeferred<Unit>()
        val file = File("photo.jpg")
        coEvery { files.download(photo) } coAnswers { gate.await(); file }
        coEvery { files.saveToDevice(file, photo) } returns true
        val vm = AttachmentViewModel(files)

        vm.save(photo)
        advanceUntilIdle()
        assertTrue(vm.busy.value) // the viewer disables Save while this is true
        vm.save(photo)
        gate.complete(Unit)
        advanceUntilIdle()

        coVerify(exactly = 1) { files.download(photo) }
        coVerify(exactly = 1) { files.saveToDevice(file, photo) }
        assertFalse(vm.busy.value)
        assertEquals(AttachmentEvent.Message(R.string.chat_attachment_saved), vm.events.first())
    }

    @Test
    fun `a failed save reports the failure text`() = runTest(dispatcher) {
        coEvery { files.download(photo) } throws java.io.IOException("offline")
        val vm = AttachmentViewModel(files)

        vm.save(photo)
        advanceUntilIdle()

        assertFalse(vm.busy.value)
        assertEquals(AttachmentEvent.Message(R.string.chat_attachment_save_failed), vm.events.first())
    }
}
