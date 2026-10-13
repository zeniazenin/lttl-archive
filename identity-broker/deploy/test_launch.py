"""Offline tests of cloud selection, immutable references, and secret boundaries."""
import base64
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('lttl_cloud_launch', HERE / 'launch.py')
launch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launch)
MASTER = base64.urlsafe_b64encode(bytes(range(32))).decode().rstrip('=')
OIDC = 'example-client-secret +/= with intentional spaces '


def config(provider):
    return json.loads((HERE / (provider + '.example.json')).read_text())


class CloudLaunchTests(unittest.TestCase):
    def test_all_providers_retrieve_exact_versions_without_exposing_values(self):
        for provider in ['azure', 'aws', 'gcp']:
            with self.subTest(provider=provider):
                cfg, calls = config(provider), []
                def runner(args, env):
                    calls.append(args)
                    self.assertNotIn('LTTL_BROKER_MASTER_KEY', env)
                    self.assertNotIn('LTTL_OIDC_CLIENT_SECRET', env)
                    self.assertNotIn(MASTER, args)
                    self.assertNotIn(OIDC, args)
                    if args[1:2] == ['login']:
                        return b''
                    value = MASTER if args == launch.secret_command(provider, cfg['master_secret']) else OIDC
                    return base64.urlsafe_b64encode(value.encode()) + b'\n' if provider == 'gcp' else json.dumps(value).encode()
                env = launch.environment_for(cfg, {'PATH': '/bin', 'LTTL_BROKER_MASTER_KEY': 'stale', 'LTTL_OIDC_CLIENT_SECRET': 'stale'}, runner)
                self.assertEqual(env['LTTL_BROKER_MASTER_KEY'], MASTER)
                self.assertEqual(env['LTTL_OIDC_CLIENT_SECRET'], OIDC)
                self.assertEqual(len(calls), 3 if provider == 'azure' else 2)
                self.assertEqual(env['PATH'], '/bin')

    def test_mutable_or_malformed_references_are_rejected(self):
        cases = [('azure', 'uri', 'https://your-vault.vault.azure.net/secrets/master'),
                 ('azure', 'uri', 'http://your-vault.vault.azure.net/secrets/master/' + '1' * 32),
                 ('azure', 'uri', 'https://attacker.example/secrets/master/' + '1' * 32),
                 ('aws', 'version_id', 'AWSCURRENT'), ('aws', 'region', '--endpoint-url=http://evil'),
                 ('aws', 'arn', 'arn:aws:secretsmanager:us-west-2:123456789012:secret:name'),
                 ('gcp', 'version', 'latest'), ('gcp', 'secret', '../../secrets'),
                 ('gcp', 'project', '--access-token-file=x')]
        for provider, field, value in cases:
            with self.subTest(provider=provider, field=field):
                reference = config(provider)['master_secret']
                reference[field] = value
                with self.assertRaises(launch.SetupError):
                    launch.secret_command(provider, reference)

    def test_invalid_master_never_reaches_broker(self):
        for master in ['short', MASTER + '\n', 'A' * 42 + 'B', {'key': MASTER}, None]:
            with self.subTest(master_type=type(master).__name__):
                with self.assertRaises(launch.SetupError):
                    launch.environment_for(config('aws'), {}, lambda *_: json.dumps(master).encode())

    def test_failure_has_no_fallback_or_secret_output(self):
        stdout, stderr = io.StringIO(), io.StringIO()
        result = subprocess.CompletedProcess(['aws'], 1, stdout=MASTER.encode(), stderr=OIDC.encode())
        with patch.object(launch.shutil, 'which', return_value='/bin/aws'), patch.object(launch.subprocess, 'run', return_value=result):
            with contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                with self.assertRaises(launch.SetupError) as failure:
                    launch.environment_for(config('aws'), {'LTTL_BROKER_MASTER_KEY': MASTER})
        self.assertNotIn(MASTER, str(failure.exception) + stdout.getvalue() + stderr.getvalue())
        self.assertNotIn(OIDC, str(failure.exception) + stdout.getvalue() + stderr.getvalue())

    def test_cli_missing_and_timeout_are_sanitized(self):
        with patch.object(launch.shutil, 'which', return_value=None):
            with self.assertRaises(launch.SetupError):
                launch.run_cli(['aws'], {})
        with patch.object(launch.shutil, 'which', return_value='/bin/aws'), patch.object(launch.subprocess, 'run', side_effect=subprocess.TimeoutExpired(['aws'], 60, output=MASTER)):
            with self.assertRaises(launch.SetupError) as failure:
                launch.run_cli(['aws'], {})
        self.assertNotIn(MASTER, str(failure.exception))

    def test_no_shell_and_no_implicit_stdin(self):
        result = subprocess.CompletedProcess(['aws'], 0, stdout=b'"value"', stderr=b'')
        with patch.object(launch.shutil, 'which', return_value='/bin/aws'), patch.object(launch.subprocess, 'run', return_value=result) as run:
            self.assertEqual(launch.run_cli(['aws', 'test'], {'PATH': '/bin'}), b'"value"')
            self.assertFalse(run.call_args.kwargs.get('shell', False))
            self.assertEqual(run.call_args.kwargs['stdin'], subprocess.DEVNULL)
            self.assertEqual(run.call_args.args[0], ['/bin/aws', 'test'])

    def test_secret_payload_validation(self):
        for payload in [b'not-json', b'null', b'{}', b'""', b'"bad\\u0000secret"']:
            with self.assertRaises(launch.SetupError):
                launch.read_secret('aws', config('aws')['master_secret'], {}, lambda *_: payload)
        for payload in [b'@@@', base64.b64encode(b'\xff')]:
            with self.assertRaises(launch.SetupError):
                launch.read_secret('gcp', config('gcp')['master_secret'], {}, lambda *_: payload)

    def test_config_validation_does_not_fetch_secrets(self):
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            broker = json.loads((HERE / 'broker.example.json').read_text())
            broker['database'] = str(root / 'keys.sqlite')
            (root / 'broker.json').write_text(json.dumps(broker))
            path = root / 'deployment.json'
            path.write_text(json.dumps(config('gcp')))
            with patch.object(launch, 'run_cli', side_effect=AssertionError('unexpected cloud call')):
                loaded, target = launch.load_config(path)
            self.assertEqual(loaded['provider'], 'gcp')
            self.assertEqual(target, root / 'broker.json')
            for field, bad in [('public_url', 'http://keys.example'), ('issuer', 'http://idp.example'),
                               ('database', 'relative.db'), ('listen', '0.0.0.0:8787')]:
                changed = dict(broker, **{field: bad})
                (root / 'broker.json').write_text(json.dumps(changed))
                with self.assertRaises(launch.SetupError):
                    launch.load_config(path)


if __name__ == '__main__':
    unittest.main()
