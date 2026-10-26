package io.wolftown.kaiku.testutil

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.job
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope

/**
 * App-lifetime scope for code that launches its own long-lived work (e.g.
 * `SessionTeardownRunner`), driven by this test's scheduler.
 *
 * Unlike [TestScope.backgroundScope], its tasks count as foreground work, so
 * `advanceUntilIdle()` runs them (kotlinx-coroutines-test stops
 * `advanceUntilIdle` once only background tasks are left). It is a child of
 * the background scope's job, so `runTest` does not wait for it and cancels
 * it at the end of the test.
 */
fun TestScope.appScope(): CoroutineScope =
    CoroutineScope(Job(backgroundScope.coroutineContext.job) + StandardTestDispatcher(testScheduler))
