"""Local protocol + actual CLI integration tests. Requires Python cryptography.

Uses ephemeral loopback services, RSA-signed OIDC tokens, and disposable keys.
Never contacts a real tenant or bypasses token verification in the application.
"""
import argparse
import base64
import contextlib
import hashlib
import http.server
import json
import os
from pathlib import Path
import secrets
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import padding, rsa

ROOT = Path(__file__).resolve().parents[1]
CHECKS = []


def check(condition, name):
    if not condition:
        raise AssertionError(name)
    CHECKS.append(name)


def b64(data):
    return base64.urlsafe_b64encode(data).decode().rstrip('=')


def identifier():
    return b64(secrets.token_bytes(32))


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


HTTP = urllib.request.build_opener(NoRedirect, urllib.request.ProxyHandler({}))


def request(url, data=None, token=None, method=None):
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    req = urllib.request.Request(url, None if data is None else json.dumps(data).encode(), headers, method=method)
    try:
        response = HTTP.open(req, timeout=15)
    except urllib.error.HTTPError as e:
        response = e
    with response:
        body = response.read()
        try:
            body = json.loads(body)
        except (ValueError, UnicodeDecodeError):
            body = body.decode(errors='replace')
        return response.status, response.headers, body


class Provider(http.server.BaseHTTPRequestHandler):
    key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
    subject = 'alice'
    mode = 'valid'
    codes = {}
    authentications = 0
    origin = ''

    def log_message(self, *args):
        pass

    def send_json(self, value, status=200):
        raw = json.dumps(value).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self):
        url = urllib.parse.urlsplit(self.path)
        if url.path == '/.well-known/openid-configuration':
            self.send_json({'issuer': self.origin, 'authorization_endpoint': self.origin + '/authorize',
                            'token_endpoint': self.origin + '/token', 'jwks_uri': self.origin + '/jwks',
                            'response_types_supported': ['code'], 'subject_types_supported': ['public'],
                            'id_token_signing_alg_values_supported': ['RS256'],
                            'token_endpoint_auth_methods_supported': ['client_secret_post'],
                            'code_challenge_methods_supported': ['S256']})
        elif url.path == '/jwks':
            numbers = self.key.public_key().public_numbers()
            self.send_json({'keys': [{'kty': 'RSA', 'kid': 'local-test', 'use': 'sig', 'alg': 'RS256',
                                     'n': b64(numbers.n.to_bytes(256, 'big')),
                                     'e': b64(numbers.e.to_bytes(3, 'big'))}]})
        elif url.path == '/authorize':
            q = dict(urllib.parse.parse_qsl(url.query))
            assert q['max_age'] == '0' and q['code_challenge_method'] == 'S256'
            assert q['response_type'] == 'code' and q['client_id'] == 'local-client'
            code = identifier()
            Provider.codes[code] = (q, self.subject, self.mode)
            Provider.authentications += 1
            query = {'state': q['state'], 'code': code}
            if self.mode == 'policy-denied':
                query = {'state': q['state'], 'error': 'access_denied'}
            self.send_response(302)
            self.send_header('Location', q['redirect_uri'] + '?' + urllib.parse.urlencode(query))
            self.end_headers()
        else:
            self.send_json({}, 404)

    def do_POST(self):
        q = dict(urllib.parse.parse_qsl(self.rfile.read(int(self.headers['Content-Length'])).decode()))
        flow, subject, mode = Provider.codes.pop(q['code'])
        assert q['client_id'] == 'local-client' and q['client_secret'] == 'local-secret'
        assert b64(hashlib.sha256(q['code_verifier'].encode()).digest()) == flow['code_challenge']
        assert q['redirect_uri'] == flow['redirect_uri']
        t = int(time.time())
        claims = {'iss': self.origin, 'sub': subject, 'aud': 'local-client', 'iat': t, 'exp': t + 180,
                  'auth_time': t, 'nonce': flow['nonce'], 'name': subject,
                  'email': 'same-display-email@example.test', 'email_verified': True}
        changes = {'issuer': ('iss', 'https://wrong.example'), 'audience': ('aud', 'wrong-client'),
                   'nonce': ('nonce', 'wrong'), 'expired': ('exp', t - 3600),
                   'stale-auth': ('auth_time', t - 3600), 'future-auth': ('auth_time', t + 3600),
                   'future-issued': ('iat', t + 3600)}
        if mode in changes:
            field, value = changes[mode]
            claims[field] = value
        if mode == 'missing-auth':
            del claims['auth_time']
        raw = b64(json.dumps({'alg': 'RS256', 'kid': 'local-test'}).encode()) + '.' + b64(json.dumps(claims).encode())
        signature = self.key.sign(raw.encode(), padding.PKCS1v15(), hashes.SHA256())
        if mode == 'signature':
            signature = bytes([signature[0] ^ 1]) + signature[1:]
        self.send_json({'access_token': 'mock-access-token', 'token_type': 'Bearer', 'expires_in': 180,
                        'id_token': raw + '.' + b64(signature)})


def browser(url, finish=False):
    for _ in range(5):
        try:
            status, headers, body = request(url)
        except OSError as error:
            raise RuntimeError('Browser connection failed at ' + urllib.parse.urlsplit(url).path + ': ' + repr(error)) from error
        if status not in (302, 303, 307):
            return status, headers, body
        url = headers['Location']
        if urllib.parse.urlsplit(url).path == '/callback' and not finish:
            return status, headers, body
    raise AssertionError('Redirect loop')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--cli', type=Path, default=ROOT / 'target/debug/lttl.exe')
    parser.add_argument('--broker', type=Path, default=ROOT / 'identity-broker/target/debug/lttl-key-broker.exe')
    args = parser.parse_args()
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Provider)
    Provider.origin = 'http://127.0.0.1:' + str(server.server_port)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    (ROOT / '.qa').mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='identity-', dir=ROOT / '.qa') as directory:
        work = Path(directory)
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        origin = f'http://127.0.0.1:{port}'
        database = work / 'keys.sqlite'
        config = work / 'broker.json'
        config.write_text(json.dumps({'public_url': origin, 'issuer': Provider.origin, 'client_id': 'local-client',
                                     'listen': f'127.0.0.1:{port}', 'database': str(database)}))
        profile = work / 'profile.json'
        profile.write_text(json.dumps({'broker_url': origin, 'provider': 'entra'}))
        env = dict(os.environ, LTTL_OIDC_CLIENT_SECRET='local-secret', LTTL_BROKER_MASTER_KEY=identifier(),
                   LTTL_MANUAL_SIGN_IN='1', TEMP=str(work), TMP=str(work))
        process = subprocess.Popen([str(args.broker), str(config)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        try:
            for _ in range(100):
                try:
                    if request(origin + '/health')[0] == 200:
                        break
                except (OSError, urllib.error.URLError):
                    if process.poll() is not None:
                        raise RuntimeError(process.stderr.read().decode())
                    time.sleep(.1)
            else:
                raise AssertionError('Broker startup timed out')

            def begin(subject, archive=None, mode='valid'):
                Provider.subject, Provider.mode = subject, mode
                verifier, state = identifier(), identifier()
                status, _, body = request(origin + '/v1/auth/start', {'archive_id': archive,
                    'redirect_uri': 'http://127.0.0.1:19999/callback', 'state': state,
                    'challenge': b64(hashlib.sha256(verifier.encode()).digest())})
                assert status == 200
                status, headers, _ = browser(body['browser_url'])
                if mode != 'valid':
                    check(status == 403, 'reject OIDC ' + mode)
                    return
                assert status == 303
                q = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(headers['Location']).query))
                assert q['state'] == state
                return q['code'], verifier

            def login(subject, archive=None):
                code, verifier = begin(subject, archive)
                status, _, body = request(origin + '/v1/auth/exchange', {'code': code, 'verifier': verifier})
                assert status == 200
                return body

            def api(path, session=None, data=None, method=None):
                return request(origin + path, data, session['token'] if session else None, method)

            def restart_broker():
                nonlocal process
                process.terminate()
                process.wait(timeout=10)
                process.stderr.close()
                process = subprocess.Popen([str(args.broker), str(config)], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
                for _ in range(100):
                    try:
                        if request(origin + '/health')[0] == 200:
                            return
                    except (OSError, urllib.error.URLError):
                        if process.poll() is not None:
                            raise RuntimeError(process.stderr.read().decode())
                        time.sleep(.1)
                raise AssertionError('Broker restart timed out')

            alice, bob, eve = login('alice'), login('bob'), login('eve')
            check(len({u['user']['id'] for u in [alice, bob, eve]}) == 3, 'same email never merges identities')
            check(api('/v1/users')[0] == 401, 'anonymous directory denied')
            check(len(api('/v1/users', alice)[2]) == 3, 'enrolled directory')
            Provider.subject, Provider.mode = 'bob', 'valid'
            proof, state = identifier(), identifier()
            status, _, body = request(origin + '/v1/auth/start', {'archive_id': None,
                'redirect_uri': 'http://127.0.0.1:19999/callback', 'state': state,
                'challenge': b64(hashlib.sha256(proof.encode()).digest())})
            assert status == 200
            restart_broker()
            status, headers, _ = browser(body['browser_url'])
            check(status == 303, 'pending login survives broker restart')
            q = dict(urllib.parse.parse_qsl(urllib.parse.urlsplit(headers['Location']).query))
            restart_broker()
            status, _, resumed = request(origin + '/v1/auth/exchange', {'code': q['code'], 'verifier': proof})
            check(status == 200, 'authorization exchange survives broker restart')
            restart_broker()
            check(api('/v1/users', resumed)[0] == 200, 'management session survives broker restart')
            check(request(origin + '/v1/auth/exchange', {'code': q['code'], 'verifier': proof})[0] == 403, 'consumed exchange stays consumed after restart')
            check(api('/v1/archives', alice, {'recipients': [identifier()]})[0] == 403, 'unknown recipient denied')
            status, _, archive = api('/v1/archives', alice, {'recipients': [bob['user']['id']]})
            check(status == 200, 'register encrypted archive key')
            archive_id = archive['archive_id']
            endpoint = '/v1/archives/' + archive_id + '/key'
            opening = login('bob', archive_id)
            check(api(endpoint, opening, method='POST')[2]['key'] == archive['key'], 'selected recipient receives key')
            restart_broker()
            check(api(endpoint, opening, method='POST')[0] == 403, 'release grant is one use')
            check(api(endpoint, opening, method='POST')[0] == 403, 'consumed opening grant stays consumed after restart')
            for who in ['alice', 'eve']:
                check(api(endpoint, login(who, archive_id), method='POST')[0] == 403, who + ' not selected denied')
            check(api(endpoint, login('bob'), method='POST')[0] == 403, 'manage grant cannot open')
            check(api(endpoint, login('bob', identifier()), method='POST')[0] == 403, 'grant bound to archive')
            check(api('/v1/archives/' + archive_id + '/revoke', eve, method='POST')[0] == 403, 'only owner can revoke')
            with contextlib.closing(sqlite3.connect(database)) as db:
                wrapped = db.execute('SELECT wrapped FROM archives WHERE id=?', (archive_id,)).fetchone()[0]
                raw_key = base64.urlsafe_b64decode(archive['key'] + '=')
                check(raw_key not in wrapped, 'database stores wrapped key')
            code, verifier = begin('bob')
            check(request(origin + '/v1/auth/exchange', {'code': code, 'verifier': identifier()})[0] == 403, 'wrong native PKCE rejected')
            check(request(origin + '/v1/auth/exchange', {'code': code, 'verifier': verifier})[0] == 403, 'failed exchange cannot replay')
            code, verifier = begin('bob')
            exchange = {'code': code, 'verifier': verifier}
            assert request(origin + '/v1/auth/exchange', exchange)[0] == 200
            check(request(origin + '/v1/auth/exchange', exchange)[0] == 403, 'successful exchange cannot replay')
            for mode in ['issuer', 'audience', 'nonce', 'expired', 'stale-auth', 'future-auth', 'future-issued', 'missing-auth', 'signature', 'policy-denied']:
                begin('bob', archive_id, mode)
            check(api('/v1/archives/' + archive_id + '/revoke', alice, method='POST')[0] == 200, 'owner revocation')
            check(api(endpoint, login('bob', archive_id), method='POST')[0] == 403, 'revocation stops fresh key release')
            disabled = login('eve', archive_id)
            with contextlib.closing(sqlite3.connect(database)) as db:
                db.execute('UPDATE users SET disabled=1 WHERE id=?', (eve['user']['id'],))
                db.commit()
            check(api(endpoint, disabled, method='POST')[0] == 403, 'disabled account session denied')
            with contextlib.closing(sqlite3.connect(database)) as db:
                db.execute('UPDATE users SET disabled=0 WHERE id=?', (eve['user']['id'],))
                db.commit()

            def cli(subject, command, success=True, signin=True):
                Provider.subject, Provider.mode = subject, 'valid'
                child = subprocess.Popen([str(args.cli), *map(str, command)], env=env,
                                         stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8')
                errors, log, signs = [], [], []
                def read_stderr():
                    for line in child.stderr:
                        log.append(line)
                        marker = 'Complete organization sign-in in your browser: '
                        if marker in line:
                            try:
                                check(browser(line.split(marker, 1)[1].strip(), finish=True)[0] == 200, 'native loopback callback')
                                signs.append(True)
                            except Exception as error:
                                errors.append(error)
                reader = threading.Thread(target=read_stderr, daemon=True)
                reader.start()
                try:
                    child.wait(timeout=90)
                except subprocess.TimeoutExpired:
                    child.kill()
                    raise AssertionError('CLI timed out')
                reader.join(timeout=5)
                output = child.stdout.read()
                child.stdout.close(); child.stderr.close()
                assert not errors, str(errors) + ' CLI exit=' + str(child.returncode) + ' log=' + ''.join(l for l in log if 'your browser:' not in l)
                assert (child.returncode == 0) == success, ''.join(log)
                assert bool(signs) == signin
                return output

            source = work / 'source'
            source.mkdir()
            (source / 'empty.txt').write_bytes(b'')
            (source / 'unicode-\u03bb.txt').write_text('Secret project documents.\n' * 5000, encoding='utf-8')
            (source / 'binary.bin').write_bytes(secrets.token_bytes(2 * 1048576 + 13))
            members = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in source.iterdir()}
            for fmt in ['zip', 'tar-zstd', 'tar-lz4']:
                output = work / (fmt + '.lttl')
                cli('alice', ['create', source, '-o', output, '--format', fmt, '--backend', 'cpu',
                              '--identity-profile', profile, '--recipient', bob['user']['id']])
                check(output.read_bytes()[:8] == b'LTTLID01', fmt + ' encrypted envelope')
                dest = work / ('extracted-' + fmt)
                cli('bob', ['extract', output, '-d', dest, '--identity-profile', profile])
                extracted = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in dest.rglob('*') if p.is_file()}
                check(extracted == members, fmt + ' actual CLI extraction hashes')
                check(len(json.loads(cli('bob', ['list', output, '--identity-profile', profile]))) >= 3, fmt + ' authenticated listing')
                cli('bob', ['test', output, '--identity-profile', profile])
                check(True, fmt + ' authenticated integrity test')
                denied = work / ('denied-' + fmt)
                cli('eve', ['extract', output, '-d', denied, '--identity-profile', profile], success=False)
                check(not denied.exists(), fmt + ' unauthorized extraction publishes nothing')
            output = work / 'tar-zstd.lttl'
            for kind in ['tampered', 'truncated', 'trailing']:
                data = bytearray(output.read_bytes())
                if kind == 'tampered':
                    data[-20] ^= 1
                elif kind == 'truncated':
                    del data[-1:]
                else:
                    data.append(0)
                altered = work / (kind + '.lttl')
                altered.write_bytes(data)
                dest = work / ('extract-' + kind)
                cli('bob', ['extract', altered, '-d', dest, '--identity-profile', profile], success=False)
                check(not dest.exists(), kind + ' archive publishes nothing')
            wrong = work / 'wrong-profile.json'
            wrong.write_text(json.dumps({'broker_url': 'https://untrusted.example', 'provider': 'entra'}))
            cli('bob', ['test', output, '--identity-profile', wrong], success=False, signin=False)
            check(True, 'archive cannot switch trusted broker')
            cli('alice', ['identity', 'revoke', output, '--profile', profile])
            cli('bob', ['test', output, '--identity-profile', profile], success=False)
            check(True, 'CLI revocation enforced')
            with contextlib.closing(sqlite3.connect(database)) as db:
                events = dict(db.execute('SELECT event,count(*) FROM audit GROUP BY event'))
            check(events.get('key-denied', 0) >= 3 and events.get('key-release', 0) >= 9, 'release and denial audit events')
            report = {'scope': 'Local RSA-signed mock OIDC and real broker/CLI; no real tenant',
                      'live_entra_validated': False, 'checks_passed': len(CHECKS), 'checks': CHECKS,
                      'authentications': Provider.authentications, 'audit_event_counts': events}
            (ROOT / 'docs/identity-validation.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
            print(f'PASS: {len(CHECKS)} checks; {Provider.authentications} fresh OIDC flows; all 3 formats hash verified.')
        finally:
            process.terminate()
            process.wait(timeout=10)
            process.stderr.close()
            server.shutdown()
            server.server_close()


if __name__ == '__main__':
    main()
