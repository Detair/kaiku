package io.wolftown.kaiku.ui.admin

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.data.api.AdminApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import javax.inject.Inject
import kotlin.coroutines.cancellation.CancellationException

/** Whether Settings shows "Admin" (`GET /api/admin/status` → `is_admin`). */
@HiltViewModel
class AdminEntryViewModel @Inject constructor(private val adminApi: AdminApi) : ViewModel() {
    private val _isAdmin = MutableStateFlow(false)
    val isAdmin: StateFlow<Boolean> = _isAdmin.asStateFlow()

    init {
        viewModelScope.launch {
            _isAdmin.value = try {
                adminApi.status()
            } catch (c: CancellationException) {
                throw c
            } catch (_: Exception) {
                false
            }
        }
    }
}
