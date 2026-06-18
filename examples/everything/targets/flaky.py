"""A deliberately flapping HTTP target: returns 503 for the first ~25 seconds of
every minute, 200 otherwise. Gives the stack a monitor that genuinely flaps so
uptime history, a real outage, an open episode, escalation/on-call paging, the
SLO burn and the tag-routed alert path all light up with REAL state changes."""
import http.server
import time


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        down = int(time.time()) % 60 < 25
        self.send_response(503 if down else 200)
        self.send_header("content-type", "text/plain")
        self.end_headers()
        self.wfile.write(b"unhealthy\n" if down else b"ok\n")

    def log_message(self, *_args):
        pass


if __name__ == "__main__":
    http.server.HTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
