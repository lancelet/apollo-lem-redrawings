#!/usr/bin/env python3
"""A tiny static server for the inspector: python3 serve.py DIR PORT.

Serves DIR (the app's files plus a `sheet` link to the bundle) on 127.0.0.1
with the MIME types the app needs (WebP, wasm) and no caching, so a rebuilt
bundle or app is picked up on reload."""
import http.server
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".webp": "image/webp",
        ".png": "image/png",
        ".wasm": "application/wasm",
        ".js": "text/javascript",
        ".json": "application/json",
        ".css": "text/css",
        ".bin": "application/octet-stream",
        ".ttf": "font/ttf",
        "": "application/octet-stream",
    }

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def log_message(self, fmt, *args):
        pass


if __name__ == "__main__":
    directory, port = sys.argv[1], int(sys.argv[2])
    handler = lambda *a, **k: Handler(*a, directory=directory, **k)
    http.server.ThreadingHTTPServer(("127.0.0.1", port), handler).serve_forever()
