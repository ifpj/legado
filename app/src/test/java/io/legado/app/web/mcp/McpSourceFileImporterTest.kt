package io.legado.app.web.mcp

import io.legado.app.data.entities.BookSource
import io.legado.app.model.jsSource.JsSourceUpsert
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.RandomAccessFile

class McpSourceFileImporterTest {

    @get:Rule
    val temporaryFolder = TemporaryFolder()

    @Test
    fun readsSingleJsonObjectFromDeviceFileWithUtf8Bom() {
        val content = read("\uFEFF  $firstSource\n") as McpSourceFileImporter.Content.Declarative

        assertEquals(1, content.sources.size)
        assertEquals("中文书源", content.sources.single().bookSourceName)
        assertEquals("https://one.test", content.sources.single().bookSourceUrl)
    }

    @Test
    fun readsJsonArrayInFileOrderWithoutUsingFileExtension() {
        val content = read("[$firstSource,$secondSource]", name = "source.js")
            as McpSourceFileImporter.Content.Declarative

        assertEquals(listOf("https://one.test", "https://two.test"), content.sources.map { it.bookSourceUrl })
    }

    @Test
    fun readsJavaScriptAsTextAndStripsBom() {
        val script = "var source = {bookSourceName: 'test', bookSourceUrl: 'https://one.test'};"
        val content = read("\uFEFF \n$script\n", name = "source.json")
            as McpSourceFileImporter.Content.JavaScript

        assertEquals(script, content.text)
    }

    @Test
    fun explicitFormatOverridesDetection() {
        val content = read("{}", format = "js") as McpSourceFileImporter.Content.JavaScript

        assertEquals("{}", content.text)
        assertReadError("var source = {};", "JSON 格式无效", format = "json")
    }

    @Test
    fun rejectsEmptyInvalidAndNonFilePaths() {
        listOf("", "  ", "relative/source.json", "file:///sdcard/source.json", "content://sources/1", "a\u0000b")
            .forEach { path ->
                assertThrows(IllegalArgumentException::class.java) {
                    McpSourceFileImporter.readContent(path)
                }
            }
        val missing = File(temporaryFolder.root, "missing.json")
        val error = assertThrows(IllegalArgumentException::class.java) {
            McpSourceFileImporter.readContent(missing.absolutePath)
        }
        assertTrue(error.message.orEmpty().contains("不存在"))
        val directoryError = assertThrows(IllegalArgumentException::class.java) {
            McpSourceFileImporter.readContent(temporaryFolder.root.absolutePath)
        }
        assertTrue(directoryError.message.orEmpty().contains("普通文件"))
    }

    @Test
    fun rejectsUnsupportedFormatBeforeReading() {
        val error = assertThrows(IllegalArgumentException::class.java) {
            McpSourceFileImporter.readContent("missing.json", "xml")
        }
        assertTrue(error.message.orEmpty().contains("format"))
    }

    @Test
    fun rejectsEmptyAndInvalidUtf8Files() {
        assertReadError("\uFEFF \r\n", "内容不能为空")
        val file = temporaryFolder.newFile()
        file.writeBytes(byteArrayOf(0xc3.toByte(), 0x28))

        val error = assertThrows(IllegalArgumentException::class.java) {
            McpSourceFileImporter.readContent(file.absolutePath)
        }
        assertTrue(error.message.orEmpty().contains("UTF-8"))
    }

    @Test
    fun validatesEveryJsonArrayEntryBeforeReturningAnythingToSave() {
        assertReadError("[$firstSource,{}]", "第 2 项：源名称和 URL 不能为空")
        assertReadError("[$firstSource,null]", "第 2 项必须是 BookSource JSON 对象")
        assertReadError("[$firstSource,[]]", "第 2 项必须是 BookSource JSON 对象")
        assertReadError("[]", "数组不能为空")
        assertReadError("true", "必须是 BookSource 对象或数组", format = "json")
        assertReadError("[$firstSource,]", "JSON 格式无效")
    }

    @Test
    fun rejectsMainJsInJsonConsistentlyWithSaveSource() {
        assertReadError(
            """{"bookSourceName":"test","bookSourceUrl":"https://one.test","mainJs":"secret script"}""",
            "format=js",
        )
    }

    @Test
    fun doesNotEchoInvalidJsonOrScriptBodies() {
        val invalid = """{"bookSourceName":"secret source body","bookSourceUrl":null}"""
        val error = assertThrows(IllegalArgumentException::class.java) { read(invalid) }

        assertFalse(error.message.orEmpty().contains("secret source body"))
        assertReadError("{secret source body", "JSON 格式无效")
    }

    @Test
    fun boundsFileBytesBeforeParsing() {
        val file = temporaryFolder.newFile()
        RandomAccessFile(file, "rw").use { it.setLength(McpSourceFileImporter.MAX_FILE_BYTES + 1L) }

        val error = assertThrows(IllegalArgumentException::class.java) {
            McpSourceFileImporter.readContent(file.absolutePath)
        }
        assertTrue(error.message.orEmpty().contains("10 MiB"))
    }

    @Test
    fun enforcesIndividualSourceByteLimitForJsonAndJavaScript() {
        val oversizedText = "中".repeat(JsSourceUpsert.MAX_SOURCE_BYTES / 3 + 1)
        assertReadError(oversizedText, "1 MiB")
        assertReadError(
            """[{"bookSourceName":"test","bookSourceUrl":"https://one.test","bookSourceComment":"$oversizedText"}]""",
            "1 MiB",
        )
    }

    @Test
    fun allowsBatchLargerThanSingleSourceLimit() {
        val comment = "a".repeat(600_000)
        val first = firstSource.dropLast(1) + ",\"bookSourceComment\":\"$comment\"}"
        val second = secondSource.dropLast(1) + ",\"bookSourceComment\":\"$comment\"}"
        val content = read("[$first,$second]") as McpSourceFileImporter.Content.Declarative

        assertEquals(2, content.sources.size)
    }

    @Test
    fun returnsOnlyBoundedNameAndUrlSummary() {
        val source = BookSource(
            bookSourceName = "test",
            bookSourceUrl = "https://one.test",
            mainJs = "secret script",
            bookSourceComment = "secret comment",
        )
        val summary = McpSourceFileImporter.summarize(listOf(source))

        assertTrue(summary.startsWith("已导入 1 个书源"))
        assertTrue(summary.contains(source.bookSourceUrl))
        assertFalse(summary.contains("secret"))
        assertFalse(summary.contains("mainJs"))
        val longSummary = McpSourceFileImporter.summarize(List(1_000) { source })
        assertTrue(longSummary.startsWith("已导入 1000 个书源"))
        assertTrue(longSummary.length < 10_100)
        assertTrue(longSummary.contains("已截断"))
    }

    private fun read(text: String, format: String? = null, name: String? = null): McpSourceFileImporter.Content {
        val file = if (name == null) temporaryFolder.newFile() else temporaryFolder.newFile(name)
        file.writeText(text, Charsets.UTF_8)
        return McpSourceFileImporter.readContent(file.absolutePath, format)
    }

    private fun assertReadError(text: String, message: String, format: String? = null) {
        val error = assertThrows(IllegalArgumentException::class.java) { read(text, format) }
        assertTrue(error.message, error.message.orEmpty().contains(message))
    }

    companion object {
        private const val firstSource = """{"bookSourceName":"中文书源","bookSourceUrl":"https://one.test"}"""
        private const val secondSource = """{"bookSourceName":"second","bookSourceUrl":"https://two.test"}"""
    }
}
