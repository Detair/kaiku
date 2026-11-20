package io.wolftown.kaiku.ui.dm

import androidx.annotation.StringRes
import androidx.lifecycle.SavedStateHandle
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.repository.DmRepository
import io.wolftown.kaiku.data.repository.PeopleRepository
import io.wolftown.kaiku.domain.Person
import io.wolftown.kaiku.domain.matches
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import java.util.logging.Logger
import javax.inject.Inject

/** What "New message" shows. */
data class NewMessageUi(
    val loading: Boolean = true,
    /** Everyone loaded; [shown] is the search result. */
    val people: List<Person> = emptyList(),
    val query: String = "",
    /** Servers whose member list failed (some people may be missing). */
    val failedServers: Int = 0,
    /** Whole list failed: the full-screen error with "Retry". */
    @StringRes val loadError: Int? = null,
    /** The person whose conversation is being opened; every row is disabled meanwhile. */
    val openingUserId: String? = null,
    /** A failed open, shown once (snackbar). */
    @StringRes val message: Int? = null,
) {
    val shown: List<Person> get() = if (query.isBlank()) people else people.filter { it.matches(query) }

    /** "Some people could not be loaded" with Retry: whenever a server failed, also with nobody loaded. */
    val partialFailure: Boolean get() = failedServers > 0 && loadError == null
}

/**
 * "New message": everyone on the user's servers, searchable; picking a person
 * opens the DM with them, created on the server if there is none yet.
 *
 * The list is a look at one moment. A tap therefore asks again whether the
 * person can still be written to ([PeopleRepository.confirm]) before any
 * conversation is opened, also one that already exists: someone who left
 * the user's servers or was blocked meanwhile, on this or another device,
 * is taken off the list instead.
 */
@HiltViewModel
class NewMessageViewModel @Inject constructor(
    private val peopleRepository: PeopleRepository,
    private val dmRepository: DmRepository,
    private val savedState: SavedStateHandle = SavedStateHandle(),
) : ViewModel() {

    private val _ui = MutableStateFlow(NewMessageUi(query = savedState.get<String>(QUERY_KEY).orEmpty()))
    val ui: StateFlow<NewMessageUi> = _ui.asStateFlow()

    private val _openConversation = Channel<String>(Channel.BUFFERED)

    /** Channel id of the conversation to show, once per pick. */
    val openConversation = _openConversation.receiveAsFlow()

    private var loadJob: Job? = null

    init {
        load()
        // Who can be written to may have changed (a block on any device, or the connection was
        // away): what is shown is loaded again.
        viewModelScope.launch { peopleRepository.invalidated.collect { load() } }
    }

    fun load() {
        loadJob?.cancel()
        _ui.update { it.copy(loading = true, loadError = null) }
        loadJob = viewModelScope.launch {
            try {
                val result = peopleRepository.load()
                _ui.update { it.copy(loading = false, people = result.people, failedServers = result.failedServers) }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                logger.warning("people load failed (${e::class.simpleName})")
                // Nobody unconfirmed stays tappable (someone may have left a server meanwhile).
                _ui.update { it.copy(loading = false, people = emptyList(), failedServers = 0, loadError = DmErrorText.load(e)) }
            }
        }
    }

    fun search(query: String) {
        // Kept across the process being recreated.
        savedState[QUERY_KEY] = query
        _ui.update { it.copy(query = query) }
    }

    /** Opens the conversation with [person]. Ignored while another one is opening (double tap). */
    fun open(person: Person) {
        var start = false
        _ui.update {
            start = false
            if (it.openingUserId != null) it else {
                start = true
                it.copy(openingUserId = person.userId, message = null)
            }
        }
        if (!start) return
        viewModelScope.launch {
            try {
                val confirmed = peopleRepository.confirm(person.userId)
                if (confirmed == null) {
                    // No longer someone to write to: gone from the list, with the neutral sentence.
                    _ui.update {
                        it.copy(people = it.people.filter { p -> p.userId != person.userId }, message = R.string.dm_new_not_possible)
                    }
                    return@launch
                }
                val channelId = dmRepository.openDirectMessage(confirmed)
                // Like a tap in the DM list: the open DM is read, and its new messages count as read.
                dmRepository.setActiveChannel(channelId)
                dmRepository.markReadInBackground(channelId)
                _openConversation.send(channelId)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // Also when the person could not be checked: they stay, and a tap tries again.
                logger.warning("open conversation failed (${e::class.simpleName})")
                _ui.update { it.copy(message = DmErrorText.open(e)) }
            } finally {
                _ui.update { it.copy(openingUserId = null) }
            }
        }
    }

    fun messageShown() = _ui.update { it.copy(message = null) }

    private companion object {
        val logger: Logger = Logger.getLogger("NewMessageViewModel")
        const val QUERY_KEY = "query"
    }
}
