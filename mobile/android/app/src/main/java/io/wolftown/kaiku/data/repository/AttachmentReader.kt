package io.wolftown.kaiku.data.repository

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import dagger.hilt.android.qualifiers.ApplicationContext
import javax.inject.Inject
import javax.inject.Singleton

/** Reads a picked file through the `ContentResolver`; a seam so tests need no Android. */
interface AttachmentReader {
    /** Size in bytes as reported by the provider, or null if unknown. */
    fun size(uri: Uri): Long?

    fun displayName(uri: Uri): String?

    fun mimeType(uri: Uri): String?

    /** Reads at most [maxBytes] bytes; null if the file cannot be opened. */
    fun read(uri: Uri, maxBytes: Long): ByteArray?
}

@Singleton
class ContentResolverAttachmentReader @Inject constructor(
    @ApplicationContext private val context: Context
) : AttachmentReader {

    override fun size(uri: Uri): Long? = queryColumn(uri, OpenableColumns.SIZE) { c, i ->
        if (c.isNull(i)) null else c.getLong(i).takeIf { it >= 0 }
    }

    override fun displayName(uri: Uri): String? = queryColumn(uri, OpenableColumns.DISPLAY_NAME) { c, i ->
        c.getString(i)
    }

    override fun mimeType(uri: Uri): String? = context.contentResolver.getType(uri)

    override fun read(uri: Uri, maxBytes: Long): ByteArray? {
        val input = context.contentResolver.openInputStream(uri) ?: return null
        return input.use { stream ->
            val out = java.io.ByteArrayOutputStream()
            val buffer = ByteArray(BUFFER_SIZE)
            var total = 0L
            while (total < maxBytes) {
                val n = stream.read(buffer, 0, minOf(buffer.size.toLong(), maxBytes - total).toInt())
                if (n < 0) break
                out.write(buffer, 0, n)
                total += n
            }
            out.toByteArray()
        }
    }

    private fun <T> queryColumn(uri: Uri, column: String, extract: (android.database.Cursor, Int) -> T?): T? =
        context.contentResolver.query(uri, arrayOf(column), null, null, null)?.use { cursor ->
            val index = cursor.getColumnIndex(column)
            if (index >= 0 && cursor.moveToFirst()) extract(cursor, index) else null
        }

    private companion object {
        const val BUFFER_SIZE = 64 * 1024
    }
}
