package io.wolftown.kaiku.testutil

import io.wolftown.kaiku.data.local.AuthState
import io.wolftown.kaiku.data.session.SessionTeardownRunner
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob

/** A teardown runner with no steps, never started; for tests that only need an AuthRepository. */
fun noTeardown(authState: AuthState): SessionTeardownRunner =
    SessionTeardownRunner(emptySet(), authState, CoroutineScope(SupervisorJob()))
