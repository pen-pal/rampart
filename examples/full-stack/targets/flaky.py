"""A deliberately flaky HTTP target: returns 503 for the first ~20 seconds of
every minute, 200 otherwise. Gives the example a monitor that genuinely flaps,
so uptime history, a real outage, alerting and (with an escalation policy) the
paging path all light up."""
import http.server
import time


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        down = int(time.time()) % 60 < 20
        self.send_response(503 if down else 200)
        self.send_header("content-type", "text/plain")
        self.end_headers()
        self.wfile.write(b"unhealthy\n" if down else b"ok\n")

    def log_message(self, *_args):
        pass


if __name__ == "__main__":
    http.server.HTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
