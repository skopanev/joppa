"""Local NTK contract fixture. Production never imports this provider."""
import json
import secrets
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

class Provider:
    def __init__(self):
        self.user = 'ntk-owner'
        self.attempts = {}
        self.claimed = set()
        provider = self
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass
            def reply(self, value, status=200):
                self.send_response(status)
                self.send_header('Content-Type', 'application/json')
                self.end_headers()
                self.wfile.write(json.dumps(value).encode())
            def do_POST(self):
                if self.path == '/device/start':
                    code, secret = secrets.token_hex(12), secrets.token_hex(16)
                    provider.attempts[code] = {'secret': secret, 'approved': False}
                    self.reply({'code': code, 'device_secret': secret, 'expires_in': 300})
                elif self.path == '/device/poll':
                    data = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    attempt = provider.attempts.get(data['code'])
                    if not attempt or attempt['secret'] != data['device_secret'] or data['code'] in provider.claimed:
                        self.reply({'error': 'invalid device secret'}, 403)
                    elif not attempt['approved']:
                        self.reply({'status': 'pending'}, 202)
                    else:
                        provider.claimed.add(data['code'])
                        self.reply({'status': 'ready', 'key': 'private-test-ntk-key', 'user_id': 'untrusted-hint'})
                else:
                    self.reply({'error': 'not found'}, 404)
            def do_GET(self):
                if self.path.startswith('/link?'):
                    code = parse_qs(urlparse(self.path).query)['code'][0]
                    provider.attempts[code]['approved'] = True
                    self.reply({'signed_in': True})
                elif self.path == '/v1/me' and self.headers.get('Authorization') == 'Bearer private-test-ntk-key':
                    self.reply({'user_id': provider.user, 'workspaces': []})
                else:
                    self.reply({'error': 'unauthorized'}, 401)
        self.server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        self.url = f'http://127.0.0.1:{self.server.server_port}'
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()
