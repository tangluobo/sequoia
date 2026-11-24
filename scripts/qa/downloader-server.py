"""Owned HTTP range fixture. Never proxies or contacts external hosts."""
import argparse
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

parser = argparse.ArgumentParser()
parser.add_argument('--port-file', type=Path, required=True)
parser.add_argument('--payload-file', type=Path, required=True)
args = parser.parse_args()
payload = (bytes(range(251)) * 492)[:123456]
args.payload_file.write_bytes(payload)

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *unused):
        pass

    def serve(self, body):
        if self.path != '/fixture.bin':
            self.send_error(404)
            return
        start, end = 0, len(payload) - 1
        part = self.headers.get('Range')
        if part:
            unit, bounds = part.split('=', 1)
            first, last = bounds.split('-', 1)
            if unit != 'bytes' or not first.isdigit() or (last and not last.isdigit()):
                self.send_error(416)
                return
            start = int(first)
            end = min(int(last), end) if last else end
            if not 0 <= start <= end < len(payload):
                self.send_error(416)
                return
        self.send_response(206 if part else 200)
        self.send_header('Content-Type', 'application/octet-stream')
        self.send_header('Content-Length', str(end - start + 1))
        self.send_header('Accept-Ranges', 'bytes')
        self.send_header('ETag', '"owned-downloader-v1"')
        if part:
            self.send_header('Content-Range', f'bytes {start}-{end}/{len(payload)}')
        self.end_headers()
        if body:
            self.wfile.write(payload[start:end + 1])

    def do_HEAD(self):
        self.serve(False)

    def do_GET(self):
        self.serve(True)

server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
args.port_file.write_text(str(server.server_port), encoding='ascii')
server.serve_forever(poll_interval=0.1)
