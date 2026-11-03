package io.wolftown.kaiku.data.api

/**
 * The shared HTTP client's read timeout and the long-poll `wait` that fits under
 * it (C26 enrolment status, C27 attempt poll: the server allows 0..25 s). The
 * server answers early on any change, so the wait only bounds idle round trips;
 * a wait at or above the read timeout would end every idle poll in a timeout.
 */
object LongPoll {
    /** OkHttp's read timeout, set explicitly on the client (KaikuHttpClient). */
    const val READ_TIMEOUT_SECONDS = 10L

    /** Every long-poll's `wait`: below [READ_TIMEOUT_SECONDS] with room for the round trip. */
    const val WAIT_SECONDS = 8
}
