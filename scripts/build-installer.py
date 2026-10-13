"""Package an existing Windows x64 release with portable NSIS (no system install)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--nsis', type=Path, default=ROOT/'tools/nsis-3.12/makensis.exe')
    p.add_argument('--crt', type=Path, help='Visual Studio x64 Microsoft.VC143.CRT redistributable directory')
    p.add_argument('--version', default='0.1.0-alpha.1')
    args = p.parse_args()
    if not all(c.isalnum() or c in '.-' for c in args.version):
        raise SystemExit('Invalid version')
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
        for src in (ROOT/'docs').glob('*.md'):
            copy(src, Path('docs')/src.name)
        for src in (ROOT/'identity-broker/deploy').glob('*.md'):
            copy(src, Path('identity-broker/deploy')/src.name)
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
    print(f'Built {output.name} ({output.stat().st_size:,} bytes); SHA-256 {digest}')

if __name__ == '__main__':
    main()
