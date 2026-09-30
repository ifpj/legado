package io.legado.app.web.mcp

import io.ktor.http.ContentType
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpStatusCode
import io.ktor.http.parseUrl
import io.ktor.server.application.Application
import io.ktor.server.application.ApplicationCallPipeline
import io.ktor.server.request.header
import io.ktor.server.response.header
import io.ktor.server.response.respondText
import io.ktor.server.routing.RoutingContext
import io.legado.app.api.controller.BookSourceController
import io.modelcontextprotocol.kotlin.sdk.server.Server
import io.modelcontextprotocol.kotlin.sdk.server.mcpStreamableHttp

fun Application.configureMcp(
    tokenRequiredProvider: () -> Boolean,
    tokenProvider: () -> String?,
    unauthorizedMessage: () -> String,
    allowedOrigins: List<String>,
    serverFactory: RoutingContext.() -> Server,
) {
    // The SDK couples Host and Origin checks. Keep the existing Origin policy
    // here so device aliases do not need to be enumerated in a Host allowlist.
    val allowedOriginHosts = allowedOrigins.mapNotNull { parseUrl(it)?.host?.lowercase() }.toSet()
    intercept(ApplicationCallPipeline.Plugins) {
        context.response.header(HttpHeaders.CacheControl, "no-store")
        if (
            tokenRequiredProvider() &&
            !BookSourceController.matchesJsSourceApiToken(
                tokenProvider(),
                context.request.header(McpAccess.TOKEN_HEADER),
            )
        ) {
            context.respondText(
                text = unauthorizedMessage(),
                status = HttpStatusCode.Unauthorized,
            )
            finish()
            return@intercept
        }
        val origin = context.request.header(HttpHeaders.Origin)
        if (origin != null && parseUrl(origin)?.host?.lowercase() !in allowedOriginHosts) {
            context.respondText(
                text = """{"error":{"code":-32000,"message":"Invalid Origin"},"jsonrpc":"2.0"}""",
                contentType = ContentType.Application.Json,
                status = HttpStatusCode.Forbidden,
            )
            finish()
        }
    }
    mcpStreamableHttp(
        path = McpAccess.PATH,
        enableDnsRebindingProtection = false,
        block = serverFactory,
    )
}
