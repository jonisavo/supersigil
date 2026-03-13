"""Tiny HTTP server that returns canned responses for testing."""

import json
from http.server import HTTPServer, BaseHTTPRequestHandler


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/ok":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"status": "ok", "count": 42}).encode())
        elif self.path == "/not-found":
            self.send_response(404)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(json.dumps({"error": "not found"}).encode())
        elif self.path == "/server-error":
            self.send_response(500)
            self.send_header("Content-Type", "text/plain")
            self.end_headers()
            self.wfile.write(b"internal server error")
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length)
        # Echo the body back with a twist
        self.send_response(201)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps({"received": True, "echo": body.decode()}).encode())

    def log_message(self, format, *args):
        pass  # Silence request logging


if __name__ == "__main__":
    server = HTTPServer(("127.0.0.1", 9876), Handler)
    print(f"Listening on http://127.0.0.1:9876", flush=True)
    server.serve_forever()
