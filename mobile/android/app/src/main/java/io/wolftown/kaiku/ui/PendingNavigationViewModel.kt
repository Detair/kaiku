package io.wolftown.kaiku.ui

import androidx.lifecycle.ViewModel
import dagger.hilt.android.lifecycle.HiltViewModel
import io.wolftown.kaiku.navigation.PendingNavigation
import javax.inject.Inject

/** Exposes the singleton [PendingNavigation] to the nav graph. */
@HiltViewModel
class PendingNavigationViewModel @Inject constructor(
    val pendingNavigation: PendingNavigation,
) : ViewModel()
