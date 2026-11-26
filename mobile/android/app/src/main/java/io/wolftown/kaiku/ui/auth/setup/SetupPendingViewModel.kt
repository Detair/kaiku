package io.wolftown.kaiku.ui.auth.setup

import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.ViewModel
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.flow.StateFlow
import javax.inject.Inject

/**
 * The waiting screen's view of the one setup ([SetupOwner]): a setup that waits (24 h for
 * an existing account, a claim whose code was not confirmed before the app closed, or a claim
 * whose answer never arrived). It survives restarts because the state is the stored record; this
 * class holds none. [check] runs at every app open and every 15 minutes in the foreground. Never
 * a login form.
 */
@HiltViewModel
class SetupPendingViewModel @Inject constructor(private val owner: SetupOwner) : ViewModel() {

    private val view = owner.attach()

    init {
        owner.openWaiting()
    }

    val ui: StateFlow<SetupUi> = owner.ui
    val linkIgnored: StateFlow<Boolean> = owner.linkIgnored
    val linkKept: StateFlow<Int?> = owner.linkKept
    val cannotAsk: StateFlow<Boolean> = owner.cannotAsk
    val discardable: StateFlow<Boolean> = owner.discardable
    val lastResort: StateFlow<Boolean> = owner.lastResort

    fun check() = owner.check()
    fun discard() = owner.discard()
    fun signIn(activity: FragmentActivity) = owner.signIn(activity)

    public override fun onCleared() {
        view.close()
        super.onCleared()
    }
}
