package io.legado.app.web.mcp

import io.legado.app.data.appDb
import io.legado.app.data.entities.BookSource
import io.legado.app.help.ConcurrentRateLimiter.Companion.concurrentRecordMap
import io.legado.app.help.config.SourceConfig
import io.legado.app.help.source.clearExploreKindsCache
import io.legado.app.model.SharedJsScope
import io.legado.app.model.jsSource.JsSourceUpsert
import io.legado.app.utils.GSON
import io.legado.app.utils.fromJsonObject
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import java.util.concurrent.Callable

internal object McpSourceStore {

    suspend fun saveDeclarative(text: String): BookSource {
        return saveDeclarativeBatch(listOf(parseDeclarative(text))).single()
    }

    suspend fun saveDeclarativeBatch(sources: List<BookSource>): List<BookSource> {
        return JsSourceUpsert.withSaveLock {
            withContext(Dispatchers.IO) {
                val saved = appDb.runInTransaction(Callable {
                    sources.map(::saveParsedDeclarative)
                })
                // Only invalidate runtime state after the entire batch commits. Complete
                // this cleanup even if the request was cancelled while the DB was writing.
                withContext(NonCancellable) {
                    val changed = saved.filter { it.changed }
                    changed.forEach { savedSource ->
                        savedSource.previous?.let { old ->
                            if (old.exploreUrl != savedSource.source.exploreUrl) {
                                old.clearExploreKindsCache()
                            }
                            if (old.jsLib != savedSource.source.jsLib) {
                                SharedJsScope.remove(old.jsLib)
                            }
                        }
                        concurrentRecordMap.remove(savedSource.source.bookSourceUrl)
                    }
                    SourceConfig.removeSources(changed.mapNotNull { it.previous?.bookSourceUrl })
                }
                saved.map { it.source }
            }
        }
    }

    private data class SavedSource(
        val source: BookSource,
        val previous: BookSource?,
        val changed: Boolean,
    )

    private fun saveParsedDeclarative(source: BookSource): SavedSource {
        val old = appDb.bookSourceDao.getBookSource(source.bookSourceUrl)
        if (!JsSourceUpsert.prepareForSave(source, old) && old != null) {
            return SavedSource(old, old, changed = false)
        }
        old?.let { appDb.bookSourceDao.delete(it) }
        appDb.bookSourceDao.insert(source)
        return SavedSource(source, old, changed = true)
    }

    internal fun parseDeclarative(text: String): BookSource {
        require(text.toByteArray(Charsets.UTF_8).size <= JsSourceUpsert.MAX_SOURCE_BYTES) {
            "书源 JSON 不能超过 1 MiB"
        }
        val json = text.dropWhile { it.isWhitespace() || it == '\uFEFF' }
        val source = GSON.fromJsonObject<BookSource>(json).getOrThrow()
        require(source.bookSourceName.isNotBlank() && source.bookSourceUrl.isNotBlank()) {
            "源名称和 URL 不能为空"
        }
        require(source.mainJs.isNullOrBlank()) {
            "带 mainJs 的书源必须使用 format=js 提交脚本原文"
        }
        source.mainJs = null
        return source
    }
}
