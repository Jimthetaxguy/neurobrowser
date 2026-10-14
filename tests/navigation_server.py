"""Deterministic loopback pages for the AppKit navigation regression."""
import http.server
import pathlib
import socketserver
import sys
import time


class Server(http.server.ThreadingHTTPServer):
    def server_bind(self):
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = self.server_address[:2]


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


with Server(('127.0.0.1', 0), Handler) as server:
    pathlib.Path(sys.argv[1]).write_text(str(server.server_port))
    server.serve_forever()
