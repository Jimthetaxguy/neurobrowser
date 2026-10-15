"""Deterministic loopback pages for the AppKit navigation regression.

`LoopbackHTTPServer` is also the listener for the WebKit capability workload.
The two request handlers stay separate.
"""
import http.server
import pathlib
import socketserver
import sys
import time


class LoopbackHTTPServer(http.server.ThreadingHTTPServer):
    """Bind without resolving 127.0.0.1.

    `HTTPServer.server_bind` calls `socket.getfqdn` on 127.0.0.1 before the
    constructor returns. On the macOS 26 GitHub runner that reverse lookup
    stalls and writes nothing to stderr, so the port file never appears. Bind
    through `TCPServer` and keep the numeric host.
    """

    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        host, port = self.server_address[:2]
        self.server_name = host
        self.server_port = port


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/failure':
            self.connection.close()
            return
        if self.path == '/redirect':
            self.send_response(302)
            self.send_header('Location', '/final')
            self.end_headers()
            return
        if self.path == '/slow':
            time.sleep(1)
        body = f'<title>{self.path}</title><p>Navigation fixture</p>'.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        try:
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, *_args):
        pass


if __name__ == '__main__':
    with LoopbackHTTPServer(('127.0.0.1', 0), Handler) as server:
        pathlib.Path(sys.argv[1]).write_text(str(server.server_port))
        server.serve_forever()
