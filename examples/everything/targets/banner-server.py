#!/usr/bin/env python3
"""Tiny multi-port banner server for the `imap` / `pop3` banner monitors.

Rampart's banner probe opens a TCP connection and asserts the greeting starts
with the protocol's expected prefix:
    imap → "* OK"
    pop3 → "+OK"

The full dovecot image needs non-trivial config to listen in plaintext, so this
purpose-built server just emits the exact greetings on 143 (IMAP) and 110 (POP3)
and then reads/echoes politely so the probe sees a real, live banner. No fixtures
— a genuine TCP server returning a genuine protocol greeting.
"""
import socket
import threading

GREETINGS = {
    143: b"* OK [CAPABILITY IMAP4rev1] rampart-demo IMAP ready\r\n",
    110: b"+OK rampart-demo POP3 server ready\r\n",
}


def serve(port: int, greeting: bytes) -> None:
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("0.0.0.0", port))
    s.listen(64)
    print(f"[banner] listening on :{port}", flush=True)
    while True:
        try:
            conn, _ = s.accept()
        except OSError:
            continue
        threading.Thread(target=handle, args=(conn, greeting), daemon=True).start()


def handle(conn: socket.socket, greeting: bytes) -> None:
    try:
        conn.sendall(greeting)
        conn.settimeout(2)
        try:
            data = conn.recv(256)
            # Minimal politeness: ack a LOGOUT/QUIT then close.
            if data:
                conn.sendall(b"+OK bye\r\n" if greeting.startswith(b"+OK") else b"* BYE\r\n")
        except OSError:
            pass
    finally:
        try:
            conn.close()
        except OSError:
            pass


def main() -> None:
    threads = [threading.Thread(target=serve, args=(p, g), daemon=True) for p, g in GREETINGS.items()]
    for t in threads:
        t.start()
    for t in threads:
        t.join()


if __name__ == "__main__":
    main()
