"""Feed the headless drill from a real torrent client inside its loopback-only sandbox."""

import hashlib
import socket
import subprocess
import threading
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

PAYLOAD = b"synthetic intake\n"


class WebSeed(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/weights.txt":
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header("Content-Length", str(len(PAYLOAD)))
        self.end_headers()
        self.wfile.write(PAYLOAD)
        self.server.served.set()

    def log_message(self, *_):
        pass


def main():
    if {name for _, name in socket.if_nameindex()} != {"lo"}:
        raise SystemExit(
            "run through test-headless-linux.sh in its private network namespace"
        )
    incoming = Path("/incoming")
    if list(incoming.iterdir()) or any(
        Path(p).exists() for p in ("/state", "/primary", "/backup")
    ):
        raise SystemExit("expected only the empty intake directory")
    # Single-file v1 torrent. Its one piece covers the entire synthetic payload.
    piece = hashlib.sha1(PAYLOAD, usedforsecurity=False).digest()
    info = (
        b"d6:lengthi17e4:name11:weights.txt12:piece lengthi16384e6:pieces20:"
        + piece
        + b"7:privatei1ee"
    )
    with HTTPServer(("127.0.0.1", 0), WebSeed) as seed:
        url = f"http://127.0.0.1:{seed.server_port}/weights.txt".encode()
        torrent = (
            b"d4:info"
            + info
            + b"8:url-list"
            + str(len(url)).encode()
            + b":"
            + url
            + b"e"
        )
        metainfo = Path("/tmp/intake.torrent")
        metainfo.write_bytes(torrent)
        seed.served = threading.Event()
        thread = threading.Thread(target=seed.serve_forever)
        thread.start()
        try:
            subprocess.run(
                [
                    "/aria2c",
                    "--no-conf=true",
                    "--dir=/incoming",
                    "--enable-color=false",
                    "--seed-time=0",
                    "--enable-dht=false",
                    "--enable-dht6=false",
                    "--enable-peer-exchange=false",
                    "--bt-enable-lpd=false",
                    "--bt-exclude-tracker=*",
                    "--file-allocation=none",
                    "--summary-interval=0",
                    str(metainfo),
                ],
                cwd="/tmp",
                timeout=20,
                check=True,
            )
        finally:
            seed.shutdown()
            thread.join()
        if not seed.served.is_set():
            raise SystemExit("client never requested the fixture from its web seed")
    if (
        list(incoming.iterdir()) != [incoming / "weights.txt"]
        or (incoming / "weights.txt").read_bytes() != PAYLOAD
    ):
        raise SystemExit("client did not leave exactly one complete, matching file")


if __name__ == "__main__":
    main()
