"""Package an existing Windows x64 release with portable NSIS (no system install)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--nsis', type=Path, default=ROOT/'tools/nsis-3.12/makensis.exe')
    p.add_argument('--crt', type=Path, help='Visual Studio x64 Microsoft.VC143.CRT redistributable directory')
    p.add_argument('--version', default=tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version'])
    p.add_argument('--smoke-test', action='store_true', help='Exercise staged binaries without replacing a registered installation')
    args = p.parse_args()
    if not all(c.isalnum() or c in '.-' for c in args.version):
        raise SystemExit('Invalid version')
    actual=subprocess.check_output([str(ROOT/'target/release/lttl.exe'),'--version'],text=True).strip()
    if actual != 'lttl '+args.version:
        raise SystemExit('Release executable version does not match installer version; rebuild first')
    if not args.nsis.is_file():
        raise SystemExit('Download the portable NSIS package or provide --nsis')
    if not args.crt:
        vswhere = Path(os.environ['ProgramFiles(x86)'])/'Microsoft Visual Studio/Installer/vswhere.exe'
        vs = Path(subprocess.check_output([str(vswhere), '-latest', '-products', '*', '-property', 'installationPath'], text=True).strip())
        candidates = sorted((vs/'VC/Redist/MSVC').glob('*/x64/Microsoft.VC143.CRT'))
        if not candidates:
            raise SystemExit('Supply --crt pointing to the MSVC x64 redistributable directory')
        args.crt = candidates[-1]
    toolchain = Path(os.environ['USERPROFILE'])/'.rustup/toolchains/stable-x86_64-pc-windows-msvc/bin'
    env = dict(os.environ, CARGO_HOME=str(ROOT/'.cargo-home'))
    env['PATH'] = str(toolchain) + os.pathsep + env['PATH']
    metadata = json.loads(subprocess.check_output([str(toolchain/'cargo.exe'), 'metadata', '--locked', '--offline', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc'], cwd=ROOT, env=env))
    dist = ROOT/'dist'
    dist.mkdir(exist_ok=True)
    output = dist/f'lttl-{args.version}-windows-x64-setup.exe'
    if output.exists():
        raise SystemExit('Output already exists; choose a new version or move the previous artifact')
    with tempfile.TemporaryDirectory(prefix='installer-', dir=ROOT/'.qa') as temp:
        stage = Path(temp)/'payload'
        stage.mkdir()
        def copy(src, dest):
            dest = stage/dest
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dest)
        copy(ROOT/'target/release/lttl.exe', 'lttl.exe')
        copy(ROOT/'target/release/lttl-gui.exe', 'lttl-gui.exe')
        copy(ROOT/'assets/lttl.ico', 'assets/lttl.ico')
        copy(ROOT/'assets/README.md', 'assets/README.md')
        for dll in args.crt.glob('*.dll'):
            copy(dll, dll.name)
        for name in ['vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll']:
            if not (stage/name).is_file():
                raise SystemExit('Required MSVC runtime missing: ' + name)
        for name in ['lttl_gpu.dll','nvcomp64_5.dll','nvcomp_cpu64_5.dll','cudart64_12.dll','LICENSE-nvcomp.txt','LICENSE-cuda-runtime.txt','NOTICE-nvcomp.txt']:
            copy(ROOT/'target/release/gpu'/name, Path('gpu')/name)
        for name in ['LICENSE','README.md','THIRD_PARTY_NOTICES.md']:
            copy(ROOT/name, name)
        for name in ['install-explorer.ps1','uninstall-explorer.ps1']:
            copy(ROOT/'scripts'/name, Path('scripts')/name)
        # Include nested guides, diagrams, validation and public serverless examples.
        # Git exclusions keep credentials, generated caches and local configs out.
        public_files=subprocess.check_output(['git','ls-files','--cached','--others','--exclude-standard','--',
            'docs','identity-broker/deploy','identity-broker/audit-vault'],cwd=ROOT,text=True).splitlines()
        for name in sorted(set(public_files)):
            src=ROOT/name
            # The final installer hash is only available after packaging; its
            # validation report belongs in the repository, not inside itself.
            if name in ['docs/windows-installer-validation.json','docs/native-installer-validation.json']: continue
            if src.is_file(): copy(src,Path(name))
        copy(args.nsis.parent/'COPYING', 'licenses/nsis/COPYING')
        rust_notices = toolchain.parent/'share/doc/rust'
        for name in ['COPYRIGHT.html','COPYRIGHT-library.html']:
            copy(rust_notices/name, Path('licenses/rust-toolchain')/name)
        for src in (rust_notices/'licenses').rglob('*'):
            if src.is_file():
                copy(src, Path('licenses/rust-toolchain/licenses')/src.relative_to(rust_notices/'licenses'))
        packages = []
        resolved = {n['id'] for n in metadata['resolve']['nodes']}
        for package in metadata['packages']:
            if package['source'] is None or package['id'] not in resolved:
                continue
            folder = Path(package['manifest_path']).parent
            license_files = []
            for src in folder.rglob('*'):
                if src.is_file() and src.name.upper().startswith(('LICENSE','COPYING','NOTICE','AUTHORS')):
                    relative = src.relative_to(folder)
                    copy(src, Path('licenses/rust')/f"{package['name']}-{package['version']}"/relative)
                    license_files.append(relative.as_posix())
            packages.append({k:package[k] for k in ['name','version','license','repository']} | {'license_files':license_files})
        (stage/'licenses/rust/packages.json').write_text(json.dumps(packages, indent=2), encoding='utf-8')
        terms = (ROOT/'THIRD_PARTY_NOTICES.md').read_text()+'\n\n'+(ROOT/'LICENSE').read_text()
        for name in ['LICENSE-nvcomp.txt','LICENSE-cuda-runtime.txt']:
            terms += '\n\nOPTIONAL NVIDIA COMPONENT — '+name+'\n\n'+(stage/'gpu'/name).read_text(encoding='utf-8')
        (stage/'licenses/INSTALLER-LICENSES.txt').write_text(terms, encoding='utf-8-sig')
        smoke = []
        if args.smoke_test:
            work = Path(temp)/'smoke'; work.mkdir()
            source = work/'input'; source.mkdir()
            (source/'empty').mkdir()
            data = b''.join(((i//16 + (i%16)*123) % 65536).to_bytes(2,'little') for i in range(160_000))
            (source/'numbers.bin').write_bytes(data)
            (source/'text.txt').write_text('Packaged client round trip\n'*1000,encoding='utf-8')
            def cli(*arguments):
                result = subprocess.run([str(stage/'lttl.exe'),*map(str,arguments)],capture_output=True,check=True,timeout=120)
                return json.loads(result.stdout)
            for index,(fmt,backend) in enumerate([('lttl','auto'),('lttl','lttl-fast'),('lttl','lttl-dense'),
                                                 ('zip','cpu'),('tar-zstd','cpu'),('tar-lz4','cpu')]):
                archive = work/f'archive-{index}'; dest = work/f'out-{index}'
                cli('create',source,'-o',archive,'--format',fmt,'--backend',backend)
                cli('test',archive); assert len(cli('list',archive)) == 4
                cli('extract',archive,'-d',dest)
                for name in ['numbers.bin','text.txt']:
                    assert (dest/'input'/name).read_bytes() == (source/name).read_bytes()
                smoke.append(f'{fmt}/{backend}: create, list, test, extract and exact-byte recovery')
        manifest = Path(temp)/'delete-files.nsh'
        def nsis_path(path):
            value = str(path).replace('/', '\\')
            if '"' in value or '$' in value:
                raise SystemExit('Unsafe NSIS path')
            return value
        lines = [f'Delete "$INSTDIR\\{nsis_path(f.relative_to(stage))}"' for f in stage.rglob('*') if f.is_file()]
        dirs = sorted([d for d in stage.rglob('*') if d.is_dir()],key=lambda d:len(d.parts),reverse=True)
        lines += [f'RMDir "$INSTDIR\\{nsis_path(d.relative_to(stage))}"' for d in dirs]
        manifest.write_text('\n'.join(lines)+'\n', encoding='utf-8-sig')
        subprocess.run([str(args.nsis.resolve()), '/V2', '/DVERSION='+args.version, '/DPAYLOAD='+str(stage),
                        '/DOUTPUT='+str(output), '/DDELETE_MANIFEST='+str(manifest),
                        str(ROOT/'installer/windows/lttl.nsi')], check=True, cwd=ROOT)
        hashes = {f.relative_to(stage).as_posix(): hashlib.sha256(f.read_bytes()).hexdigest() for f in stage.rglob('*') if f.is_file()}
        (dist/f'lttl-{args.version}-payload-sha256.json').write_text(json.dumps(hashes,indent=2)+'\n',encoding='utf-8')
    digest = hashlib.sha256(output.read_bytes()).hexdigest()
    (dist/(output.name+'.sha256')).write_text(f'{digest}  {output.name}\n',encoding='ascii')
    if args.smoke_test:
        validation = {'installer':output.name,'sha256':digest,'binary_sha256':hashlib.sha256((ROOT/'target/release/lttl.exe').read_bytes()).hexdigest(),
                      'scope':'Smoke-tested the exact staged payload before NSIS compilation; no install/uninstall or registry changes',
                      'checks':smoke,'signed':False,'interactive_installer_tested':False,'installed_application_replaced':False}
        (ROOT/'docs/native-installer-validation.json').write_text(json.dumps(validation,indent=2)+'\n',encoding='utf-8')
    print(f'Built {output.name} ({output.stat().st_size:,} bytes); SHA-256 {digest}')

if __name__ == '__main__':
    main()
