package io.legado.app.web.mcp

import java.net.InetAddress

object McpAccess {

    const val PATH = "/mcp"
    const val TOKEN_HEADER = "X-Legado-Token"

    fun allowedOrigins(addresses: List<InetAddress>): List<String> = buildList {
        add("http://localhost")
        add("http://127.0.0.1")
        add("http://[::1]")
        addresses.mapTo(this) { "http://${it.hostAddress}" }
    }.distinct()

    fun endpointUrls(addresses: List<InetAddress>, port: Int): List<String> {
        val hosts = addresses.map { it.hostAddress }.ifEmpty { listOf("127.0.0.1") }
        return hosts.map { "http://$it:$port$PATH" }
    }
}
