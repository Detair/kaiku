package io.wolftown.kaiku.data.repository

import android.content.ContentValues
import android.content.Context
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.MediaStore
import androidx.annotation.RequiresApi
import androidx.core.content.FileProvider
import dagger.hilt.android.qualifiers.ApplicationContext
import io.wolftown.kaiku.data.api.AuthHeaderInterceptor
import io.wolftown.kaiku.data.api.ServerUrls
import io.wolftown.kaiku.data.local.TokenStorage
import io.wolftown.kaiku.domain.model.Attachment
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.TimeUnit
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Files of received attachments (AND-B-4): downloads them into the app's cache
 * (with the same bearer handling as images), hands them to other apps through
 * the [FileProvider], and saves them where the user finds them, all without a
 * storage permission: MediaStore on Android 10+, the system "save as" dialog
 * before that (the screen asks for the target, [copyTo] writes it).
 */
@Singleton
class AttachmentFiles internal constructor(
    private val context: Context,
    okHttpClient: OkHttpClient,
    private val tokenStorage: TokenStorage,
    /** Tests: answers requests after the auth interceptor, instead of the network. */
    transport: okhttp3.Interceptor?,
) {

    @Inject
    constructor(
        @ApplicationContext context: Context,
        okHttpClient: OkHttpClient,
        tokenStorage: TokenStorage,
    ) : this(context, okHttpClient, tokenStorage, null)

    private val client: OkHttpClient = okHttpClient.newBuilder()
        // The shared client has no read timeout (WebSocket); downloads must not hang forever.
        .readTimeout(DOWNLOAD_READ_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .addInterceptor(AuthHeaderInterceptor(tokenStorage))
        .apply { if (transport != null) addInterceptor(transport) }
        .build()

    /** Deletes every downloaded attachment (session teardown, via [ChatRepository.clearSession]). */
    fun clearCache() {
        baseDir.deleteRecursively()
    }

    private val baseDir: File get() = File(context.cacheDir, CACHE_DIR)

    /** The cached copy of [attachment], downloading it first when needed. */
    suspend fun download(attachment: Attachment): File = withContext(Dispatchers.IO) {
        val target = AttachmentPaths.cacheFile(baseDir, attachment.id, attachment.filename)
        if (target.isFile && (attachment.size <= 0 || target.length() == attachment.size)) return@withContext target
        val url = ServerUrls.resolve(tokenStorage.getServerUrl(), attachment.url)
            ?: throw IOException("attachment has no usable url")
        target.parentFile?.mkdirs()
        val partial = File(target.parentFile, target.name + ".part")
        var promoted = false
        try {
            client.newCall(Request.Builder().url(url).build()).execute().use { response ->
                if (!response.isSuccessful) throw IOException("download failed: ${response.code}")
                val body = response.body ?: throw IOException("empty download")
                val limit = AttachmentPaths.downloadLimit(attachment.size)
                body.byteStream().use { input ->
                    partial.outputStream().use { output -> copyLimited(input, output, limit) { !isActive } }
                }
            }
            if (!partial.renameTo(target)) throw IOException("cannot store download")
            promoted = true
        } finally {
            // Failure, size limit or cancellation: no half file stays behind.
            if (!promoted) partial.delete()
        }
        target
    }

    /** A `content://` address other apps may read once granted (open, share). */
    fun shareableUri(file: File): Uri =
        FileProvider.getUriForFile(context, "${context.packageName}$AUTHORITY_SUFFIX", file)

    /**
     * Saves [file] to the shared Pictures, Movies or Download folder (subfolder
     * "Kaiku") on Android 10+. Returns false below Android 10: the caller asks
     * for a target with the system dialog and uses [copyTo].
     */
    suspend fun saveToDevice(file: File, attachment: Attachment): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return false
        withContext(Dispatchers.IO) { insertIntoMediaStore(file, attachment) }
        return true
    }

    /** Writes [file] to [target] (from the system "save as" dialog). */
    suspend fun copyTo(file: File, target: Uri) = withContext(Dispatchers.IO) {
        val out = context.contentResolver.openOutputStream(target) ?: throw IOException("cannot open target")
        out.use { output -> file.inputStream().use { it.copyTo(output) } }
    }

    @RequiresApi(Build.VERSION_CODES.Q)
    private fun insertIntoMediaStore(file: File, attachment: Attachment) {
        val mime = attachment.mimeType.lowercase()
        val (collection, folder) = when {
            mime.startsWith("image/") ->
                MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_PICTURES
            mime.startsWith("video/") ->
                MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_MOVIES
            else ->
                MediaStore.Downloads.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY) to Environment.DIRECTORY_DOWNLOADS
        }
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, file.name)
            put(MediaStore.MediaColumns.MIME_TYPE, attachment.mimeType)
            put(MediaStore.MediaColumns.RELATIVE_PATH, "$folder/Kaiku")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val resolver = context.contentResolver
        val item = resolver.insert(collection, values) ?: throw IOException("cannot create media entry")
        try {
            val out = resolver.openOutputStream(item) ?: throw IOException("cannot open media entry")
            out.use { output -> file.inputStream().use { it.copyTo(output) } }
            resolver.update(item, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null)
        } catch (e: Exception) {
            resolver.delete(item, null, null)
            throw e
        }
    }

    private fun copyLimited(input: InputStream, output: OutputStream, limit: Long, cancelled: () -> Boolean) {
        val buffer = ByteArray(BUFFER_SIZE)
        var total = 0L
        while (true) {
            // Leaving the screen stops the download between chunks.
            if (cancelled()) throw kotlinx.coroutines.CancellationException("download cancelled")
            val n = input.read(buffer)
            if (n < 0) break
            total += n
            if (total > limit) throw IOException("download larger than announced")
            output.write(buffer, 0, n)
        }
    }

    private companion object {
        const val CACHE_DIR = "attachments"
        const val AUTHORITY_SUFFIX = ".files"
        const val DOWNLOAD_READ_TIMEOUT_SECONDS = 60L
        const val BUFFER_SIZE = 64 * 1024
    }
}

/** Where downloads go; pure so the path rules can be unit tested. */
object AttachmentPaths {
    private const val MAX_NAME = 100

    /** Without a known size a download may not exceed this. */
    const val UNKNOWN_SIZE_LIMIT = 512L * 1024 * 1024

    /** Allowed bytes for a download announced with [declaredSize] (some slack for a wrong size). */
    fun downloadLimit(declaredSize: Long): Long =
        if (declaredSize <= 0) UNKNOWN_SIZE_LIMIT else declaredSize + 1024 * 1024

    /**
     * A file name that cannot leave its folder: no path parts, no leading dots,
     * only letters, digits and `._- ` kept, at most [MAX_NAME] characters.
     */
    fun safeName(name: String, fallback: String = "file"): String {
        val last = name.substringAfterLast('/').substringAfterLast('\\')
        val cleaned = last.map { c -> if (c.isLetterOrDigit() || c in "._- ") c else '_' }
            .joinToString("")
            .trim()
            .trimStart('.')
        val extension = cleaned.substringAfterLast('.', "").take(10)
        val shortened = if (cleaned.length <= MAX_NAME) cleaned
        else cleaned.take(MAX_NAME - extension.length - 1) + "." + extension
        return shortened.ifBlank { fallback }
    }

    /** `<base>/<attachment id>/<file name>`, always inside [base]. */
    fun cacheFile(base: File, attachmentId: String, fileName: String): File {
        val dir = File(base, safeName(attachmentId, fallback = "attachment"))
        val file = File(dir, safeName(fileName))
        val canonicalBase = base.canonicalFile
        require(file.canonicalFile.toPath().startsWith(canonicalBase.toPath())) { "attachment path escapes the cache" }
        return file
    }
}
