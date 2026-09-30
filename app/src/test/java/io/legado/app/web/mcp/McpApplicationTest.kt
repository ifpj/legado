package io.legado.app.web.mcp

import io.ktor.client.request.header
import io.ktor.client.request.request
import io.ktor.client.request.setBody
import io.ktor.client.statement.bodyAsText
import io.ktor.http.ContentType
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.HttpStatusCode
import io.ktor.http.contentType
import io.ktor.server.testing.testApplication
import io.modelcontextprotocol.kotlin.sdk.server.Server
import io.modelcontextprotocol.kotlin.sdk.server.ServerOptions
import io.modelcontextprotocol.kotlin.sdk.types.Implementation
import io.modelcontextprotocol.kotlin.sdk.types.ServerCapabilities
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class McpApplicationTest {

    @Test
    fun missingOrWrongTokenRejectsEveryMcpMethodBeforeParsing() = testApplication {
        application { testMcpApplication() }

        listOf(HttpMethod.Get, HttpMethod.Post, HttpMethod.Delete).forEach { method ->
            listOf(null, "wrong").forEach { token ->
                val response = client.request(McpAccess.PATH) {
                    this.method = method
                    header(HttpHeaders.Host, "redmi-k40s.lan:1236")
                    header(HttpHeaders.Accept, "application/json, text/event-stream")
                    token?.let { header(McpAccess.TOKEN_HEADER, it) }
                    contentType(ContentType.Application.Json)
                    setBody("not-json")
                }
                assertEquals(HttpStatusCode.Unauthorized, response.status)
                assertEquals("no-store", response.headers[HttpHeaders.CacheControl])
            }
        }

        val trailingSlash = client.request("${McpAccess.PATH}/") {
            method = HttpMethod.Post
            header(HttpHeaders.Host, "redmi-k40s.lan:1236")
        }
        assertEquals(HttpStatusCode.Unauthorized, trailingSlash.status)

        val encodedPath = client.request("/m%63p") {
            method = HttpMethod.Post
            header(HttpHeaders.Host, "redmi-k40s.lan:1236")
        }
        assertEquals(HttpStatusCode.Unauthorized, encodedPath.status)
    }

    @Test
    fun validTokenReachesTheSdkRoute() = testApplication {
        application { testMcpApplication() }
        val response = client.request(McpAccess.PATH) {
            method = HttpMethod.Post
            header(HttpHeaders.Host, "localhost")
            header(HttpHeaders.Accept, "application/json, text/event-stream")
            header(McpAccess.TOKEN_HEADER, "secret")
            contentType(ContentType.Application.Json)
            setBody("not-json")
        }
        assertEquals(HttpStatusCode.BadRequest, response.status)
        assertEquals("no-store", response.headers[HttpHeaders.CacheControl])
    }

    @Test
    fun ipAndArbitraryHostnamesCanInitializeWithEitherTokenSetting() {
        listOf(true, false).forEach { tokenRequired ->
            testApplication {
                application { testMcpApplication(tokenRequired) }
                listOf(
                    "localhost:1236",
                    "192.168.123.158:1236",
                    "redmi-k40s.lan:1236",
                    "reader.local:1236",
                    "other-device.example:1236",
                ).forEach { host ->
                    val response = client.request(McpAccess.PATH) {
                        method = HttpMethod.Post
                        header(HttpHeaders.Host, host)
                        header(HttpHeaders.Accept, "application/json, text/event-stream")
                        if (tokenRequired) header(McpAccess.TOKEN_HEADER, "secret")
                        contentType(ContentType.Application.Json)
                        setBody(
                            """{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"""
                        )
                    }
                    assertEquals(host, HttpStatusCode.OK, response.status)
                    assertTrue(host, response.bodyAsText().contains("\"serverInfo\""))
                    assertEquals("no-store", response.headers[HttpHeaders.CacheControl])
                }
            }
        }
    }

    @Test
    fun allowedOriginStillMatchesHostIgnoringCaseSchemeAndPort() = testApplication {
        application { testMcpApplication() }
        listOf("http://localhost", "https://LOCALHOST:9876").forEach { origin ->
            val response = client.request(McpAccess.PATH) {
                method = HttpMethod.Post
                header(HttpHeaders.Host, "redmi-k40s.lan:1236")
                header(HttpHeaders.Origin, origin)
                header(HttpHeaders.Accept, "application/json, text/event-stream")
                header(McpAccess.TOKEN_HEADER, "secret")
                contentType(ContentType.Application.Json)
                setBody("not-json")
            }
            assertEquals(origin, HttpStatusCode.BadRequest, response.status)
        }
    }

    @Test
    fun untrustedOrMalformedOriginIsRejectedWithEitherTokenSetting() {
        listOf(true, false).forEach { tokenRequired ->
            testApplication {
                application { testMcpApplication(tokenRequired) }
                listOf(HttpMethod.Get, HttpMethod.Post, HttpMethod.Delete).forEach { method ->
                    listOf("http://example.test", "null", "", "http://[invalid").forEach { origin ->
                        val response = client.request(McpAccess.PATH) {
                            this.method = method
                            header(HttpHeaders.Host, "redmi-k40s.lan:1236")
                            header(HttpHeaders.Origin, origin)
                            header(HttpHeaders.Accept, "application/json, text/event-stream")
                            if (tokenRequired) header(McpAccess.TOKEN_HEADER, "secret")
                            contentType(ContentType.Application.Json)
                            setBody("not-json")
                        }
                        assertEquals(origin, HttpStatusCode.Forbidden, response.status)
                        assertTrue(response.bodyAsText().contains("Invalid Origin"))
                        assertEquals("no-store", response.headers[HttpHeaders.CacheControl])
                    }
                }
            }
        }
    }

    @Test
    fun tokenIsCheckedBeforeOrigin() = testApplication {
        application { testMcpApplication() }
        val response = client.request(McpAccess.PATH) {
            method = HttpMethod.Post
            header(HttpHeaders.Host, "redmi-k40s.lan:1236")
            header(HttpHeaders.Origin, "http://example.test")
            header(McpAccess.TOKEN_HEADER, "wrong")
        }
        assertEquals(HttpStatusCode.Unauthorized, response.status)
    }

    private fun io.ktor.server.application.Application.testMcpApplication(
        tokenRequired: Boolean = true,
    ) {
        configureMcp(
            tokenRequiredProvider = { tokenRequired },
            tokenProvider = { "secret" },
            unauthorizedMessage = { "unauthorized" },
            allowedOrigins = listOf("http://localhost"),
        ) {
            Server(
                serverInfo = Implementation(name = "test", version = "1"),
                options = ServerOptions(capabilities = ServerCapabilities()),
            )
        }
    }
}
