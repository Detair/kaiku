package io.wolftown.kaiku.push

/** Records enqueue/enqueueCall/cancel calls instead of talking to WorkManager. */
class FakeWakeWorkEnqueuer : WakeWorkEnqueuer {
    val enqueued = mutableListOf<WakeSignal>()
    val calls = mutableListOf<WakeSignal>()
    val cancelled = mutableListOf<String>()
    var cancelAllCount = 0

    override fun enqueue(signal: WakeSignal) {
        enqueued += signal
    }

    override fun enqueueCall(signal: WakeSignal) {
        calls += signal
    }

    override fun cancel(channelId: String) {
        cancelled += channelId
    }

    override fun cancelAll() {
        cancelAllCount++
    }
}
