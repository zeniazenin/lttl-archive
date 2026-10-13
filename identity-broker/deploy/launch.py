#!/usr/bin/env python3
"""Start one broker using pinned secrets from the selected cloud secret manager.

Uses the provider's CLI under a dedicated workload identity. Never puts secret
values in arguments, config files, diagnostics, or stdout. Linux exec replaces
the launcher so systemd signals reach the broker. --validate performs no login.
"""
import argparse
import base64
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
from urllib.parse import urlsplit


class SetupError(Exception):
    pass


def require(condition, message):
    if not condition:
        raise SetupError(message)


def exact_fields(value, fields):
    require(isinstance(value, dict) and set(value) == set(fields), 'Unexpected or missing configuration fields')


def match(pattern, value):
    return isinstance(value, str) and re.fullmatch(pattern, value) is not None


def secret_command(provider, reference):
    """Only secret references, never payloads, enter CLI arguments."""
    if provider == 'azure':
        exact_fields(reference, ['uri'])
        require(match(r'https://[a-zA-Z0-9-]{3,24}\.vault\.azure\.net/secrets/[a-zA-Z0-9-]{1,127}/[a-fA-F0-9]{32}', reference['uri']),
                'Azure requires a versioned public-cloud Key Vault secret URI')
        return ['az', 'keyvault', 'secret', 'show', '--id', reference['uri'], '--query', 'value', '--output', 'json', '--only-show-errors']
    if provider == 'aws':
        exact_fields(reference, ['region', 'arn', 'version_id'])
        require(match(r'[a-z]{2}(?:-[a-z]+)+-[0-9]', reference['region']), 'Invalid AWS region')
        require(match(r'arn:aws:secretsmanager:[a-z0-9-]+:[0-9]{12}:secret:[a-zA-Z0-9/_+=.@-]+', reference['arn']), 'AWS requires a complete public-cloud secret ARN')
        require(reference['arn'].split(':')[3] == reference['region'], 'AWS secret region mismatch')
        require(match(r'[a-zA-Z0-9-]{32,64}', reference['version_id']), 'AWS requires an immutable VersionId')
        return ['aws', 'secretsmanager', 'get-secret-value', '--region', reference['region'], '--secret-id', reference['arn'],
                '--version-id', reference['version_id'], '--query', 'SecretString', '--output', 'json', '--no-cli-pager']
    if provider == 'gcp':
        exact_fields(reference, ['project', 'secret', 'version'])
        require(match(r'[a-z][a-z0-9-]{4,61}[a-z0-9]', reference['project']), 'Invalid GCP project ID')
        require(match(r'[a-zA-Z0-9_-]{1,255}', reference['secret']), 'Invalid GCP secret name')
        require(match(r'[1-9][0-9]*', reference['version']), 'GCP requires a numeric secret version, not latest')
        return ['gcloud', 'secrets', 'versions', 'access', reference['version'], '--secret=' + reference['secret'],
                '--project=' + reference['project'], '--format=get(payload.data)', '--quiet']
    raise SetupError('Provider must be azure, aws, or gcp; self-hosted uses the broker environment directly')


def load_config(path):
    try:
        require(path.stat().st_size <= 16384, 'Deployment config is too large')
        config = json.loads(path.read_text(encoding='utf-8'))
        exact_fields(config, ['provider', 'broker_config', 'master_secret', 'oidc_secret'])
        require(isinstance(config['broker_config'], str), 'broker_config must be a path')
        broker_path = (path.parent / config['broker_config']).resolve()
        require(broker_path.stat().st_size <= 16384, 'Broker config is too large')
        broker = json.loads(broker_path.read_text(encoding='utf-8'))
        required = {'public_url', 'issuer', 'client_id', 'database', 'listen'}
        require(required <= set(broker) <= required | {'required_acr'}, 'Unexpected or missing broker config fields')
        origin = urlsplit(broker['public_url'])
        require(origin.scheme == 'https' and origin.hostname and not origin.username and not origin.password
                and origin.path in ('', '/') and not origin.query and not origin.fragment, 'Cloud broker requires an HTTPS public origin')
        issuer = urlsplit(broker['issuer'])
        require(issuer.scheme == 'https' and issuer.hostname and not issuer.username and not issuer.password
                and not issuer.query and not issuer.fragment, 'Cloud IDP issuer must use HTTPS')
        require(match(r'(127\.0\.0\.1|\[::1\]):[0-9]{1,5}', broker['listen']), 'Cloud broker must bind to loopback behind TLS proxy')
        require(0 < int(broker['listen'].rsplit(':', 1)[1]) < 65536, 'Invalid listen port')
        require(Path(broker['database']).is_absolute(), 'Use an absolute database path on persistent storage')
        require(isinstance(broker['client_id'], str) and broker['client_id'].strip(), 'Client ID is required')
        for key in ['master_secret', 'oidc_secret']:
            secret_command(config['provider'], config[key])
        require(config['master_secret'] != config['oidc_secret'], 'Use distinct secrets for master key and IDP client secret')
        return config, broker_path
    except SetupError:
        raise
    except (OSError, ValueError, TypeError, KeyError):
        raise SetupError('Cannot read valid deployment and broker configuration') from None


def run_cli(args, environment):
    executable = shutil.which(args[0], path=environment.get('PATH'))
    require(executable is not None, 'Required cloud CLI is not installed: ' + args[0])
    try:
        result = subprocess.run([executable, *args[1:]], env=environment, stdin=subprocess.DEVNULL,
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60, check=False)
    except (OSError, subprocess.SubprocessError):
        raise SetupError('Cloud CLI failed or timed out; inspect workload identity and network configuration') from None
    # Provider errors may contain sensitive material. Do not forward their output.
    require(result.returncode == 0, 'Cloud secret retrieval failed; check identity permissions, secret versions, and network access')
    require(len(result.stdout) <= 65536, 'Cloud secret response is too large')
    return result.stdout


def read_secret(provider, reference, environment, runner=run_cli):
    raw = runner(secret_command(provider, reference), environment)
    try:
        if provider == 'gcp':
            value = base64.b64decode(raw.strip(), altchars=b'-_', validate=True).decode('utf-8')
        else:
            value = json.loads(raw)
        require(isinstance(value, str) and 0 < len(value) <= 16384 and '\0' not in value, 'Cloud secret must contain a nonempty UTF-8 string')
        return value
    except (ValueError, UnicodeError):
        raise SetupError('Invalid secret payload encoding') from None


def environment_for(config, base, runner=run_cli):
    env = dict(base)
    # Existing values must never silently substitute for a failed cloud request.
    env.pop('LTTL_BROKER_MASTER_KEY', None)
    env.pop('LTTL_OIDC_CLIENT_SECRET', None)
    env.update(AWS_PAGER='', AWS_CLI_AUTO_PROMPT='off', CLOUDSDK_CORE_DISABLE_PROMPTS='1', AZURE_CORE_COLLECT_TELEMETRY='0')
    if config['provider'] == 'azure':
        runner(['az', 'login', '--identity', '--allow-no-subscriptions', '--output', 'none', '--only-show-errors'], env)
    master = read_secret(config['provider'], config['master_secret'], env, runner)
    # Strip no secret bytes: the master key must be exactly canonical base64url.
    require(match(r'[A-Za-z0-9_-]{43}', master), 'Master key must be 32 bytes in unpadded base64url encoding')
    try:
        decoded = base64.urlsafe_b64decode(master + '=')
        require(len(decoded) == 32 and base64.urlsafe_b64encode(decoded).decode().rstrip('=') == master, 'Master key is not canonical base64url')
    except ValueError:
        raise SetupError('Master key is not valid base64url') from None
    oidc = read_secret(config['provider'], config['oidc_secret'], env, runner)
    env['LTTL_BROKER_MASTER_KEY'] = master
    env['LTTL_OIDC_CLIENT_SECRET'] = oidc
    return env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--config', type=Path, required=True)
    parser.add_argument('--broker', type=Path)
    parser.add_argument('--validate', action='store_true', help='Validate configuration without cloud calls or starting the broker')
    args = parser.parse_args()
    config, broker_config = load_config(args.config.resolve())
    if args.validate:
        print('Deployment configuration valid; cloud access has not been tested.')
        return 0
    require(args.broker is not None and args.broker.is_file(), 'Supply --broker with the installed executable path')
    environment = environment_for(config, os.environ)
    executable = str(args.broker.resolve())
    if os.name == 'posix':
        os.execve(executable, [executable, str(broker_config)], environment)
    return subprocess.call([executable, str(broker_config)], env=environment)


if __name__ == '__main__':
    try:
        sys.exit(main())
    except SetupError as error:
        print('lttl deployment: ' + str(error), file=sys.stderr)
        sys.exit(1)
