package io.wolftown.kaiku.domain

/** Enrolment codes: 6 digits, shown in groups ("123 456"), typed on a keypad that never auto-submits. */
object CodeFormat {
    const val LENGTH = 6

    /** "123456" → "123 456"; other lengths are grouped in threes the same way. */
    fun group(digits: String): String = digits.chunked(3).joinToString(" ")
}
