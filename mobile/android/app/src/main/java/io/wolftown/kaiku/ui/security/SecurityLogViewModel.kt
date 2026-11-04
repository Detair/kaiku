package io.wolftown.kaiku.ui.security

import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.R
import io.wolftown.kaiku.data.api.SecurityApi
import io.wolftown.kaiku.domain.model.SecurityEvent
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

data class SecurityLogUi(
    val items: List<SecurityEvent> = emptyList(),
    val hasMore: Boolean = false,
    val loading: Boolean = false,
    @StringRes val error: Int? = null,
)

/** The read-only security log (L2-A5): newest first, older pages only on "Show older". */
@HiltViewModel
class SecurityLogViewModel @Inject constructor(private val securityApi: SecurityApi) : ViewModel() {
    private val _ui = MutableStateFlow(SecurityLogUi())
    val ui: StateFlow<SecurityLogUi> = _ui.asStateFlow()
    private var cursor: String? = null
    private var job: kotlinx.coroutines.Job? = null
    private var loaded = false

    /** The first page; a recreated screen (rotation) keeps the pages already shown. */
    fun loadOnce() {
        if (loaded) return
        load()
    }

    fun load() {
        loaded = true
        job?.cancel() // an older page still on its way never lands in the new list
        cursor = null
        _ui.value = SecurityLogUi(loading = true)
        fetch()
    }

    fun showOlder() {
        if (!_ui.value.hasMore || _ui.value.loading) return
        _ui.update { it.copy(loading = true) }
        fetch()
    }

    private fun fetch() {
        job = viewModelScope.launch {
            try {
                val page = securityApi.events(cursor, PAGE)
                cursor = page.nextCursor
                _ui.update { s ->
                    s.copy(items = (s.items + page.items).distinctBy { it.id }, hasMore = page.hasMore && page.nextCursor != null,
                        loading = false, error = null)
                }
            } catch (c: CancellationException) {
                throw c
            } catch (_: Exception) {
                _ui.update { it.copy(loading = false, error = R.string.security_network_error) }
            }
        }
    }

    private companion object {
        const val PAGE = 50
    }
}
