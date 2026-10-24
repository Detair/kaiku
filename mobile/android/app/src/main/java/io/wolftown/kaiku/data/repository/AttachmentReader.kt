package io.wolftown.kaiku.data.repository

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import dagger.hilt.android.qualifiers.ApplicationContext
import java.io.InputStream
import javax.inject.Inject
import javax.inject.Singleton

/** Reads a picked file through the `ContentResolver`; a seam so tests need no Android. All methods block. */
interface AttachmentReader {
    /** Size in bytes as reported by the provider, or null if unknown. */
    fun size(uri: Uri): Long?

    fun displayName(uri: Uri): String?

    fun mimeType(uri: Uri): String?

    /** Opens the file for streaming; null if it cannot be opened. Blocking: call off the main thread. */
    fun open(uri: Uri): InputStream?
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

    /** Provider type, else inferred from the file extension, else null. */
    override fun mimeType(uri: Uri): String? =
        context.contentResolver.getType(uri)?.takeIf { it.isNotBlank() }
            ?: displayName(uri)?.substringAfterLast('.', "")?.lowercase()
                ?.takeIf { it.isNotEmpty() }
                ?.let { MimeTypeMap.getSingleton().getMimeTypeFromExtension(it) }

    override fun open(uri: Uri): InputStream? =
        runCatching { context.contentResolver.openInputStream(uri) }.getOrNull()

    private fun <T> queryColumn(uri: Uri, column: String, extract: (android.database.Cursor, Int) -> T?): T? =
        context.contentResolver.query(uri, arrayOf(column), null, null, null)?.use { cursor ->
            val index = cursor.getColumnIndex(column)
            if (index >= 0 && cursor.moveToFirst()) extract(cursor, index) else null
        }
}
