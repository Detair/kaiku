package io.wolftown.kaiku.domain

/** Enrolment codes: 6 digits, shown in groups ("123 456"), typed on a keypad that never auto-submits. */
object CodeFormat {
    const val LENGTH = 6

    /** "123456" → "123 456"; other lengths are grouped in threes the same way. */
    fun group(digits: String): String = digits.chunked(3).joinToString(" ")

    /** The keypad's input: digits only, at most [LENGTH]; anything else changes nothing. */
    fun accept(current: String, key: Char): String =
        if (key in '0'..'9' && current.length < LENGTH) current + key else current

    /** The keypad's "Delete" key. */
    fun delete(current: String): String = current.dropLast(1)

    fun isComplete(current: String): Boolean = current.length == LENGTH && current.all { it in '0'..'9' }

    /** A value typed or pasted in one go, filtered the same way. */
    fun sanitize(value: String): String = value.filter { it in '0'..'9' }.take(LENGTH)
}
