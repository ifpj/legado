"""Offline HTTP smoke checks for the packaged service; no official account/API calls."""
import argparse
import gzip
import http.client
import json
from urllib.parse import urlsplit


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", default="http://127.0.0.1:19670")
    parser.add_argument("--token", default="")
    parser.add_argument("--arch", choices=("amd64", "arm64"))
    args = parser.parse_args()
    url = urlsplit(args.base)
    connection = http.client.HTTPConnection(url.hostname, url.port, timeout=15)

    def request(path, payload=None, authenticated=True, extra_headers=None):
        headers = {"Accept-Encoding": "gzip", "X-Relay-Client": "Container smoke"}
        if args.token and authenticated:
            headers["Authorization"] = "Bearer " + args.token
        if payload is not None:
            headers["Content-Type"] = "application/json"
            payload = json.dumps(payload, ensure_ascii=False).encode()
        headers.update(extra_headers or {})
        connection.request("GET" if payload is None else "POST", path, payload, headers)
        response = connection.getresponse()
        body = response.read()
        if response.getheader("Content-Encoding") == "gzip":
            body = gzip.decompress(body)
        return response.status, dict(response.getheaders()), body.decode()

    def get_json(path, extra_headers=None):
        status, _, body = request(path, extra_headers=extra_headers)
        assert status == 200, (path, status, body)
        return json.loads(body)

    try:
        assert get_json("/health")["status"] == "ok"
        assert "prefers-color-scheme:dark" in request("/app.css")[2]
        assert "color-scheme" in request("/")[2]
        assert "番茄书评与讨论" in request("/community")[2]
        if args.token:
            assert request("/admin/status", authenticated=False)[0] == 401
            assert request("/source.json", authenticated=False)[0] == 401
        service = get_json("/admin/status")["service"]
        if args.arch:
            assert service["platform"] == "linux"
            assert service["arch"] == {"amd64": "x86_64", "arm64": "aarch64"}[args.arch]
        assert service["memory"]["residentBytes"] > 0
        assert not service["responseCache"]
        # External host/port differ from the internal listener; exports must follow each request.
        for headers, expected in [
            ({"Host": "relay.lan:122"}, "http://relay.lan:122"),
            ({"Host": "1.1.1.1:8088"}, "http://1.1.1.1:8088"),
            ({"Host": "[::1]:122"}, "http://[::1]:122"),
            ({"Host": "container:19670", "X-Forwarded-Host": "books.example:8443", "X-Forwarded-Proto": "https"}, "https://books.example:8443"),
            ({"Host": "container:19670", "Forwarded": 'for=192.0.2.1;host="books.example";proto=https'}, "https://books.example"),
        ]:
            generated = get_json("/source.json", headers)[0]
            assert generated["bookSourceUrl"] == expected + "/fanqie"
            assert ("var RELAY_URL = " + json.dumps(expected) + ";") in generated["mainJs"]
            assert expected + '"' in request("/source.js", extra_headers=headers)[2]
            assert get_json("/admin/status", headers)["service"]["publicUrl"] == expected
            assert request("/qr.svg", extra_headers=headers)[0] == 200
        assert request("/source.json", extra_headers={"Host": "relay.lan/path"})[0] == 400
        assert request("/source.json", extra_headers={"X-Forwarded-Proto": "file"})[0] == 400
        source = get_json("/source.json?base=http%3A%2F%2Frelay.example%3A19670")[0]
        assert source["bookSourceUrl"] == "http://relay.example:19670/fanqie"
        assert source["eventListener"] and source["ruleContent"]["maxBatchSize"] == 30
        assert "relayOnEvent" in source["mainJs"]
        assert ("var RELAY_TOKEN = " + json.dumps(args.token) + ";") in source["mainJs"]
        assert request("/source.json?base=file%3A%2F%2F%2Fetc%2Fpasswd")[0] == 400
        chapter = {"title": "Smoke", "item_id": 6883749008234250760,
                   "chapter_word_number": 1234, "custom_extra": {"retained": True}}
        payload = {"operation": "transform_chapters", "args": {"data": {"item_data_list": [chapter]}},
                   "account": {"sessionCookie": "synthetic-cookie", "ttToken": "synthetic-token"}}
        status, headers, body = request("/v1/call", payload)
        assert status == 200
        result = json.loads(body)[0]
        assert result["url"].endswith("6883749008234250760")
        metadata = json.loads(json.loads(result["variable"])["fanqie"])
        assert metadata["item_id"] == "6883749008234250760"
        assert metadata["custom_extra"]["retained"]
        request_id = next(v for k, v in headers.items() if k.lower() == "x-fanqie-request-id")
        detail = get_json("/admin/requests/" + request_id)
        assert "synthetic-cookie" not in json.dumps(detail)
        assert "synthetic-token" not in json.dumps(detail)
        payload["args"]["data"]["item_data_list"] = [chapter] * 1000
        status, headers, body = request("/v1/call", payload)
        assert status == 200 and len(json.loads(body)) == 1000
        assert {k.lower(): v for k, v in headers.items()}["content-encoding"] == "gzip"
        print("Container smoke passed:", service["platform"], service["arch"],
              "token=" + str(bool(args.token)), "automatic host/port/HTTPS, embedded source/UI, metadata, redaction, gzip")
    finally:
        connection.close()


if __name__ == "__main__":
    main()
