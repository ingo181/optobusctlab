"""Mini-HTTP-Server für die Screenshot-Hilfsseite (siehe README.md).

Liefert `frame.html` aus diesem Verzeichnis aus und unter `/slow?ms=N` ein
1x1-GIF, das erst nach N Millisekunden kommt - das hält das `load`-Ereignis
der Hilfsseite auf, bis die eingebettete App gerendert hat.

Aufruf: python3 -I helper_server.py <port>   (nur Standardbibliothek)
"""

import base64
import http.server
import pathlib
import sys
import time
import urllib.parse

FRAME = (pathlib.Path(__file__).resolve().parent / "frame.html").read_bytes()
GIF = base64.b64decode("R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7")


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        url = urllib.parse.urlparse(self.path)
        if url.path == "/slow":
            ms = int(urllib.parse.parse_qs(url.query).get("ms", ["8000"])[0])
            time.sleep(ms / 1000)
            body, ctype = GIF, "image/gif"
        else:
            body, ctype = FRAME, "text/html; charset=utf-8"
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1])
    http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
