"""Run the ignored shared-state contract against disposable PostgreSQL with TLS.

Supply an existing official PostgreSQL bin directory. No system installation,
service registration, cloud credentials or trust-store changes are performed.
Requires Python cryptography, PostgreSQL and the repository Rust toolchain.
"""
import argparse
import datetime
import ipaddress
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID

ROOT = Path(__file__).resolve().parents[1]


def run(args, **kwargs):
    return subprocess.run([str(a) for a in args], check=True, cwd=ROOT,
                          creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0, **kwargs)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pg-bin', type=Path, required=True)
    args = parser.parse_args()
    suffix = '.exe' if os.name == 'nt' else ''
    pg = lambda name: args.pg_bin.resolve() / (name + suffix)
    for name in ['initdb', 'pg_ctl']:
        if not pg(name).is_file():
            raise SystemExit('PostgreSQL bin directory is incomplete')
    with tempfile.TemporaryDirectory(prefix='postgres-', dir=ROOT / '.qa') as tmp:
        work = Path(tmp)
        password = secrets.token_urlsafe(32)
        pw = work / 'password'
        pw.write_text(password, encoding='ascii')
        run([pg('initdb'), '-D', work / 'data', '-U', 'lttltest', '--pwfile=' + str(pw),
             '-A', 'scram-sha-256', '--no-locale', '--encoding=UTF8'], stdout=subprocess.DEVNULL)
        pw.unlink()
        key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        ca_key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
        now = datetime.datetime.now(datetime.timezone.utc)
        ca_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'Disposable lttl test CA')])
        ca = (x509.CertificateBuilder().subject_name(ca_name).issuer_name(ca_name)
              .public_key(ca_key.public_key()).serial_number(x509.random_serial_number())
              .not_valid_before(now-datetime.timedelta(minutes=1)).not_valid_after(now+datetime.timedelta(days=1))
              .add_extension(x509.BasicConstraints(ca=True, path_length=0), critical=True).sign(ca_key, hashes.SHA256()))
        cert = (x509.CertificateBuilder().subject_name(x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, 'localhost')]))
                .issuer_name(ca_name).public_key(key.public_key()).serial_number(x509.random_serial_number())
                .not_valid_before(now-datetime.timedelta(minutes=1)).not_valid_after(now+datetime.timedelta(days=1))
                .add_extension(x509.SubjectAlternativeName([x509.DNSName('localhost'), x509.IPAddress(ipaddress.ip_address('127.0.0.1'))]), critical=False)
                .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True).sign(ca_key, hashes.SHA256()))
        (work/'ca.pem').write_bytes(ca.public_bytes(serialization.Encoding.PEM))
        (work/'data/server.crt').write_bytes(cert.public_bytes(serialization.Encoding.PEM))
        (work/'data/server.key').write_bytes(key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))
        (work/'data/server.key').chmod(0o600)
        with (work/'data/postgresql.conf').open('a', encoding='utf-8') as f:
            f.write("\nssl=on\nssl_cert_file='server.crt'\nssl_key_file='server.key'\n")
        (work/'data/pg_hba.conf').write_text('hostssl all all 127.0.0.1/32 scram-sha-256\n', encoding='ascii')
        with socket.socket() as sock:
            sock.bind(('127.0.0.1', 0))
            port = sock.getsockname()[1]
        started = False
        try:
            run([pg('pg_ctl'), '-D', work/'data', '-l', work/'server.log', '-o', f'-h 127.0.0.1 -p {port}', '-w', 'start'], stdout=subprocess.DEVNULL)
            started = True
            from urllib.parse import urlencode
            url = f'postgresql://lttltest:{password}@127.0.0.1:{port}/postgres?' + urlencode({'sslmode': 'verify-full', 'sslrootcert': str(work/'ca.pem')})
            env = dict(os.environ, LTTL_DATABASE_URL=url, TEMP=str(work), TMP=str(work))
            if os.name == 'nt':
                toolchain = Path(os.environ['USERPROFILE'])/'.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin'
                env['PATH'] = str(toolchain) + os.pathsep + env['PATH']
                env['CARGO_HOME'] = str(ROOT/'.cargo-home')
                cargo = toolchain/'cargo.exe'
            else:
                cargo = 'cargo'
            result = subprocess.run([str(cargo), 'test', '--locked', '--offline', '--manifest-path', 'identity-broker/Cargo.toml',
                                     'postgres_shared_state_contract', '--', '--ignored'], cwd=ROOT, env=env, capture_output=True, text=True)
            print(result.stdout, end='')
            print(result.stderr, end='')
            result.check_returncode()
            version = subprocess.check_output([str(pg('pg_ctl')), '--version'], text=True).strip()
            (ROOT/'docs/identity-postgres-validation.json').write_text(json.dumps({
                'scope': 'Disposable local PostgreSQL, verified TLS and password authentication; no managed cloud database',
                'postgres': version, 'tls': 'verify-full with ephemeral trusted CA and IP SAN',
                'shared_state_contract_passed': True,
                'checks': ['schema initialization', 'independent connection pools', 'one-use login launch',
                           'atomic concurrent code/session consumption', 'expiry', 'distinct issuer/subject identities',
                           'recipient authorization', 'owner-only revocation', 'disabled recipient rejection', 'audit writes',
                           'archive tracking policy and events across pools', 'owner-only tracking access', 'webhook outbox lease claim']
            }, indent=2)+'\n', encoding='utf-8')
        finally:
            if started:
                run([pg('pg_ctl'), '-D', work/'data', '-m', 'fast', '-w', 'stop'], stdout=subprocess.DEVNULL)


if __name__ == '__main__':
    main()
