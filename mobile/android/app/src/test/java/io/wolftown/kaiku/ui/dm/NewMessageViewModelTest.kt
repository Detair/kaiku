package io.wolftown.kaiku.ui.dm

import app.cash.turbine.test
import io.ktor.http.HttpStatusCode
import io.mockk.coEvery
import io.mockk.every
import io.mockk.coVerify
import io.mockk.mockk
import io.mockk.verify
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.ApiException
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.People
import io.wolftown.kaiku.data.repository.PeopleRepository
import io.wolftown.kaiku.data.repository.PeopleUnavailableException
import io.wolftown.kaiku.domain.Person
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Before
import org.junit.Test
import java.io.IOException

@OptIn(ExperimentalCoroutinesApi::class)
class NewMessageViewModelTest {
    private val dispatcher = StandardTestDispatcher()
    private val invalidated = kotlinx.coroutines.flow.MutableSharedFlow<Unit>(extraBufferCapacity = 1)
    private val sessions = io.wolftown.kaiku.data.session.FakeSessions("me")
    private val people: PeopleRepository = mockk {
        every { invalidated } returns this@NewMessageViewModelTest.invalidated
        // By default whoever is tapped is still someone to write to.
        coEvery { confirm(any()) } answers {
            io.wolftown.kaiku.data.repository.EligiblePerson(firstArg(), sessions.generation.enter()!!)
        }
    }
    private val dms: DmRepository = mockk(relaxed = true)
    private val anna = Person("anna", "Anna", "anna", null)
    private val bert = Person("bert", "Bert", "bert", null)

    @Before fun setUp() = Dispatchers.setMain(dispatcher)
    @After fun tearDown() = Dispatchers.resetMain()

    @Test
    fun `loads, then filters by search`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna, bert), failedServers = 1)
        val vm = NewMessageViewModel(people, dms)
        assertEquals(true, vm.ui.value.loading)
        advanceUntilIdle()

        assertFalse(vm.ui.value.loading)
        assertEquals(1, vm.ui.value.failedServers)
        vm.search("BER")
        assertEquals(listOf(bert), vm.ui.value.shown)
        vm.search("zzz")
        assertEquals(emptyList<Person>(), vm.ui.value.shown)
    }

    @Test
    fun `offline shows the network error and retry loads again`() = runTest(dispatcher) {
        coEvery { people.load() } throws PeopleUnavailableException(IOException())
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()
        assertEquals(R.string.errors_network, vm.ui.value.loadError)

        coEvery { people.load() } returns People(listOf(anna), 0)
        vm.load()
        advanceUntilIdle()
        assertNull(vm.ui.value.loadError)
        assertEquals(listOf(anna), vm.ui.value.people)
    }

    @Test
    fun `picking a person opens the conversation once, even on a double tap`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna, bert), 0)
        val answer = CompletableDeferred<String>()
        coEvery { dms.openDirectMessage(match { it.userId == "anna" }) } coAnswers { answer.await() }
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()

        vm.openConversation.test {
            vm.open(anna)
            vm.open(anna)
            vm.open(bert)
            advanceUntilIdle()
            assertEquals("anna", vm.ui.value.openingUserId)
            answer.complete("dm-anna")
            assertEquals("dm-anna", awaitItem())
        }
        advanceUntilIdle()
        assertNull(vm.ui.value.openingUserId)
        coVerify(exactly = 1) { dms.openDirectMessage(any()) }
        verify { dms.setActiveChannel("dm-anna") }
    }

    @Test
    fun `a blocked person gives the neutral message and the list stays usable`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna), 0)
        coEvery { dms.openDirectMessage(match { it.userId == "anna" }) } throws ApiException(HttpStatusCode.BadRequest, "Cannot create DM", code = "VALIDATION_ERROR")
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()

        vm.open(anna)
        advanceUntilIdle()

        assertEquals(R.string.dm_new_not_possible, vm.ui.value.message)
        assertNull(vm.ui.value.openingUserId)
        vm.messageShown()
        assertNull(vm.ui.value.message)
    }

    @Test
    fun `opening an existing conversation marks it read like the chat list`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna), failedServers = 0)
        coEvery { dms.openDirectMessage(match { it.userId == "anna" }) } returns "dm-anna"
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()

        vm.open(anna)
        advanceUntilIdle()

        verify { dms.setActiveChannel("dm-anna") }
        verify { dms.markReadInBackground("dm-anna") }
    }

    @Test
    fun `a failed server shows the notice with Retry also when nobody could be loaded`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(emptyList(), failedServers = 1)
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()

        assertEquals(true, vm.ui.value.partialFailure)
    }

    @Test
    fun `the search text survives the process being recreated`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna, bert), failedServers = 0)
        val saved = androidx.lifecycle.SavedStateHandle()
        NewMessageViewModel(people, dms, saved).search("ber")

        val restored = NewMessageViewModel(people, dms, saved)
        advanceUntilIdle()
        assertEquals("ber", restored.ui.value.query)
        assertEquals(listOf(bert), restored.ui.value.shown)
    }

    @Test
    fun `a failed reload drops the people of the earlier load`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna, bert), failedServers = 0)
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()
        assertEquals(2, vm.ui.value.people.size)

        // The user left a server meanwhile; the reload fails: nobody unconfirmed stays tappable.
        coEvery { people.load() } throws PeopleUnavailableException(IOException())
        vm.load()
        advanceUntilIdle()
        assertEquals(emptyList<Person>(), vm.ui.value.people)
        assertEquals(true, vm.ui.value.loadError != null)
    }

    // -- The list is a look at one moment ------------------------------------------------------

    @Test
    fun `someone who can no longer be written to is not opened, also when the conversation exists`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna, bert), failedServers = 0)
        // Since the list was shown, the user left Anna's server (or blocked her) on another device.
        coEvery { people.confirm("anna") } returns null
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()

        vm.open(anna)
        advanceUntilIdle()

        coVerify(exactly = 0) { dms.openDirectMessage(any()) }
        assertEquals(io.wolftown.kaiku.R.string.dm_new_not_possible, vm.ui.value.message)
        assertEquals("gone from the list", listOf(bert), vm.ui.value.people)
        assertEquals(null, vm.ui.value.openingUserId)
    }

    @Test
    fun `a tap opens only who was just confirmed`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna), failedServers = 0)
        coEvery { dms.openDirectMessage(any()) } returns "dm-anna"
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()

        vm.open(anna)
        advanceUntilIdle()

        coVerify(exactly = 1) { people.confirm("anna") }
        coVerify(exactly = 1) { dms.openDirectMessage(match { it.userId == "anna" }) }
    }

    @Test
    fun `a block on another device or a new connection reloads the list that is shown`() = runTest(dispatcher) {
        coEvery { people.load() } returns People(listOf(anna, bert), failedServers = 0)
        val vm = NewMessageViewModel(people, dms)
        advanceUntilIdle()
        assertEquals(listOf(anna, bert), vm.ui.value.people)

        // Anna was blocked on the PC.
        coEvery { people.load() } returns People(listOf(bert), failedServers = 0)
        invalidated.emit(Unit)
        advanceUntilIdle()

        assertEquals(listOf(bert), vm.ui.value.people)
    }
}
