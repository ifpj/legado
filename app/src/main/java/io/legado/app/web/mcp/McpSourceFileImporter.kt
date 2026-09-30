package io.legado.app.web.mcp

import com.google.gson.JsonElement
import io.legado.app.api.controller.BookSourceController
import io.legado.app.data.entities.BookSource
import io.legado.app.model.jsSource.JsSourceUpsert
import io.legado.app.utils.GSONStrict
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.ByteBuffer
import java.nio.charset.CharacterCodingException

internal object McpSourceFileImporter {

    const val MAX_FILE_BYTES = 10 * 1024 * 1024
    private const val SUMMARY_LIMIT = 10_000

    internal sealed interface Content {
        data class Declarative(val sources: List<BookSource>) : Content
        data class JavaScript(val text: String) : Content
    }

    suspend fun importFile(path: String, format: String?): String = withContext(Dispatchers.IO) {
        val sources = when (val content = readContent(path, format)) {
            is Content.Declarative -> McpSourceStore.saveDeclarativeBatch(content.sources)
            is Content.JavaScript -> {
                val result = BookSourceController.saveJsSource(content.text)
                require(result.isSuccess) { result.errorMsg }
                listOf(result.data as BookSource)
            }
        }
        summarize(sources)
    }

    internal fun readContent(path: String, format: String? = null): Content {
        require(format == null || format == "js" || format == "json") {
            "参数 format 必须为 js 或 json"
        }
        require(path.isNotBlank()) { "参数 path 不能为空" }
        require('\u0000' !in path) { "参数 path 包含无效字符" }
        val file = File(path)
        require(file.isAbsolute) { "参数 path 必须是 Android 设备上的绝对文件路径" }
        require(file.exists()) { "书源文件不存在，请确认路径位于运行 MCP 服务的 Android 设备上" }
        require(file.isFile) { "参数 path 必须指向普通文件" }
        require(file.canRead()) { "没有读取书源文件的权限" }
        require(file.length() <= MAX_FILE_BYTES) { "书源文件不能超过 10 MiB" }

        // Limit the actual stream as well: the file may grow after the size check.
        val bytes = file.inputStream().use { input ->
            val output = ByteArrayOutputStream()
            val buffer = ByteArray(DEFAULT_BUFFER_SIZE)
            while (true) {
                val count = input.read(buffer, 0, minOf(buffer.size, MAX_FILE_BYTES - output.size() + 1))
                if (count == -1) break
                require(output.size() + count <= MAX_FILE_BYTES) { "书源文件不能超过 10 MiB" }
                output.write(buffer, 0, count)
            }
            output.toByteArray()
        }
        val text = try {
            Charsets.UTF_8.newDecoder().decode(ByteBuffer.wrap(bytes)).toString()
        } catch (_: CharacterCodingException) {
            throw IllegalArgumentException("书源文件必须使用 UTF-8 编码")
        }.dropWhile { it.isWhitespace() || it == '\uFEFF' }.trimEnd()
        require(text.isNotEmpty()) { "书源文件内容不能为空" }

        return when (format ?: McpFormat.detectFormat(text)) {
            "json" -> Content.Declarative(parseDeclarativeFile(text))
            else -> {
                require(JsSourceUpsert.validatePayload(text) == null) { "JS源脚本不能超过 1 MiB" }
                Content.JavaScript(text)
            }
        }
    }

    private fun parseDeclarativeFile(text: String): List<BookSource> {
        val json = try {
            GSONStrict.fromJson(text, JsonElement::class.java)
        } catch (_: Exception) {
            throw IllegalArgumentException("书源文件 JSON 格式无效")
        }
        val items = when {
            json == null -> throw IllegalArgumentException("书源文件 JSON 不能为空")
            json.isJsonObject -> listOf(json)
            json.isJsonArray -> json.asJsonArray.toList()
            else -> throw IllegalArgumentException("书源文件 JSON 必须是 BookSource 对象或数组")
        }
        require(items.isNotEmpty()) { "书源文件数组不能为空" }
        return items.mapIndexed { index, item ->
            require(item.isJsonObject) { "第 ${index + 1} 项必须是 BookSource JSON 对象" }
            try {
                McpSourceStore.parseDeclarative(item.toString())
            } catch (error: Exception) {
                // Do not echo source bodies through parser error messages.
                val detail = if (error is IllegalArgumentException &&
                    error.javaClass == IllegalArgumentException::class.java
                ) error.message else "书源 JSON 字段格式无效"
                throw IllegalArgumentException("第 ${index + 1} 项：$detail")
            }
        }
    }

    internal fun summarize(sources: List<BookSource>): String {
        val summaries = sources.map {
            mapOf("bookSourceName" to it.bookSourceName, "bookSourceUrl" to it.bookSourceUrl)
        }
        return "已导入 ${sources.size} 个书源\n" +
            McpFormat.truncate(McpFormat.toPrettyJson(summaries), SUMMARY_LIMIT)
    }
}
