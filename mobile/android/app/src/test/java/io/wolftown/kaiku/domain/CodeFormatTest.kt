package io.wolftown.kaiku.domain

import org.junit.Assert.assertEquals
import org.junit.Test

class CodeFormatTest {
    @Test fun `group puts a space after three digits`() {
        assertEquals("123 456", CodeFormat.group("123456"))
        assertEquals("12", CodeFormat.group("12"))
        assertEquals("123 4", CodeFormat.group("1234"))
        assertEquals("", CodeFormat.group(""))
    }

    @Test fun `keypad accepts only six digits`() {
        var v = ""
        for (c in "12a3 4-56789") v = CodeFormat.accept(v, c)
        assertEquals("123456", v)
    }

    @Test fun `keypad deletes and corrects before confirm`() {
        var v = "12345"
        v = CodeFormat.delete(v)
        v = CodeFormat.accept(v, '9')
        v = CodeFormat.accept(v, '0')
        assertEquals("123490", v)
        assertEquals("", CodeFormat.delete(""))
        assertEquals(true, CodeFormat.isComplete(v))
        assertEquals(false, CodeFormat.isComplete("12349"))
    }
}
