"""Install into a disposable workspace directory, exercise the app and uninstall.

Uses all-users install registration and Start Menu shortcuts temporarily. Does
not select Explorer integration or touch archives/profiles outside the test.
Refuses to run while an existing lttl installation is registered.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import winreg
import sys
import tomllib

ROOT=Path(__file__).resolve().parents[1]
REG=r'Software\Microsoft\Windows\CurrentVersion\Uninstall\lttl'
def registered():
    try:
        with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,REG,0,winreg.KEY_READ|winreg.KEY_WOW64_64KEY) as key:
            return winreg.QueryValueEx(key,'InstallLocation')[0]
    except FileNotFoundError:
        return None

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('installer',type=Path)
    parser.add_argument('--manifest',type=Path,required=True)
    parser.add_argument('--report',type=Path,default=ROOT/'docs/windows-installer-validation.json')
    parser.add_argument('--identity-broker',type=Path,help='Run audit/retention integration using the installed CPU client')
    args=parser.parse_args()
    if registered(): raise SystemExit('Existing lttl installation found; refusing to modify it')
    setup=args.installer.resolve()
    expected=json.loads(args.manifest.read_text())
    checks=[]
    with tempfile.TemporaryDirectory(prefix='installer-test-',dir=ROOT/'.qa') as tmp:
        work=Path(tmp)
        unregistered=work/'unregistered'
        unregistered.mkdir()
        (unregistered/'lttl.exe').write_bytes(b'preserve this unregistered file')
        rejected=subprocess.run(f'"{setup}" /S /D={unregistered}',timeout=180,creationflags=subprocess.CREATE_NO_WINDOW)
        assert rejected.returncode==2,'Unregistered executable was not rejected'
        assert (unregistered/'lttl.exe').read_bytes()==b'preserve this unregistered file'
        assert registered() is None
        checks.append('Unregistered executable rejected without modifying files or registration')
        for nvidia in [False,True]:
            install=work/('app nvidia' if nvidia else 'app cpu')
            # NSIS /D must be the last argument, with an unquoted path. No shell.
            command=f'"{setup}" /S '+('/NVIDIA ' if nvidia else '')+f'/D={install}'
            subprocess.run(command,check=True,timeout=180,creationflags=subprocess.CREATE_NO_WINDOW)
            try:
                assert registered()==str(install),'Wrong installation directory'
                for name,digest in expected.items():
                    target=install/name
                    if name.startswith('gpu/') and not nvidia:
                        assert not target.exists(),name
                    else:
                        assert hashlib.sha256(target.read_bytes()).hexdigest()==digest,name
                checks.append(f'Payload hashes and optional component selection: nvidia={nvidia}')
                exe=install/'lttl.exe'
                def cli(*args):
                    return subprocess.check_output([str(exe),*map(str,args)],text=True,encoding='utf-8',stderr=subprocess.PIPE,timeout=60)
                assert cli('--version').strip() == 'lttl '+tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']
                checks.append('Installed CLI version')
                assert '--action' in cli('identity','retention','--help')
                assert (install/'docs/audit-retention.md').is_file() and (install/'identity-broker/audit-vault/vault.py').is_file()
                checks.append('Installed retention command, audit guide and serverless vault sources')
                if args.identity_broker and not nvidia:
                    subprocess.run([sys.executable,str(ROOT/'scripts/test-identity-flow.py'),'--tracking','--audit',
                                    '--cli',str(exe),'--broker',str(args.identity_broker.resolve())],check=True,timeout=300)
                    checks.append('Installed client passes real mock-IDP audit/retention integration')
                doctor=json.loads(cli('doctor'))
                if nvidia:
                    # Preserve result rather than asserting availability on every host.
                    (work/'doctor.json').write_text(json.dumps(doctor))
                source=work/('source-nv' if nvidia else 'source-cpu');source.mkdir()
                (source/'sample.txt').write_text('lttl installer round trip\n'*2000)
                for fmt,ext in [('lttl','lttl'),('zip','zip'),('tar-zstd','tar.zst'),('tar-lz4','tar.lz4')]:
                    archive=work/f'{source.name}.{ext}'
                    cli('create',source,'-o',archive,'--format',fmt,'--backend','cpu')
                    cli('test',archive)
                    dest=work/f'{source.name}-{fmt}'
                    cli('extract',archive,'-d',dest)
                    restored=list(dest.rglob('sample.txt'))
                    assert len(restored)==1 and restored[0].read_bytes()==(source/'sample.txt').read_bytes()
                    checks.append(f'Installed {fmt} creation/integrity/extraction: nvidia={nvidia}')
                sentinel=install/'user-kept.txt';sentinel.write_text('preserve me')
                (install/'README.md').write_text('repair this packaged file')
                repaired=subprocess.run(command,timeout=180,creationflags=subprocess.CREATE_NO_WINDOW)
                assert repaired.returncode==0,'Repair failed'
                assert hashlib.sha256((install/'README.md').read_bytes()).hexdigest()==expected['README.md']
                assert sentinel.read_text()=='preserve me'
                checks.append('Existing registered installation repaired; user file preserved')
            finally:
                subprocess.run([str(install/'Uninstall.exe'),'/S'],check=True,timeout=120,creationflags=subprocess.CREATE_NO_WINDOW)
                for _ in range(200):
                    if registered() is None and not (install/'lttl.exe').exists() and not (install/'Uninstall.exe').exists(): break
                    time.sleep(.1)
                assert registered() is None and not (install/'lttl.exe').exists() and not (install/'Uninstall.exe').exists(),'Uninstall incomplete'
            assert sentinel.read_text()=='preserve me','User file was removed'
            assert not (Path(os.environ['PROGRAMDATA'])/'Microsoft/Windows/Start Menu/Programs/lttl/lttl.lnk').exists()
            checks.append('Uninstall removes owned files/registration/shortcut and preserves user-added file')
        report={'scope':'Actual silent all-users Windows x64 installation in temporary directories, no Explorer registration',
                'checks_passed':len(checks),'checks':checks,'installer':setup.name,'sha256':hashlib.sha256(setup.read_bytes()).hexdigest(),
                'nvidia_doctor':json.loads((work/'doctor.json').read_text()),'signed':False,'visual_installer_tested':False,'explorer_install_tested':False}
        args.report.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
        print(f'PASS: {len(checks)} installer checks; both component configurations installed and uninstalled')

if __name__=='__main__': main()
