"""Sequential, verified complete-process benchmark; outputs never replace user files."""
import argparse, hashlib, json, random, shutil, statistics, subprocess, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
def sha(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest().upper()

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--runs',type=int,default=3)
    parser.add_argument('--output',default='docs/portable-benchmark-results.json')
    args=parser.parse_args()
    exe=ROOT/'target/release/lttl.exe'
    corpus=ROOT/'bench-output/auto-corpus'
    manifest=json.loads((corpus/'manifest.json').read_text(encoding='utf-8-sig'))
    configs=[('cpu-zstd-fast','tar-zstd','cpu','fast',8,None),
             ('cpu-zstd-balanced','tar-zstd','cpu','balanced',8,None),
             ('cpu-lz4-1','tar-lz4','cpu','fast',1,None),
             ('cpu-lz4-8','tar-lz4','cpu','fast',8,None),
             ('intel-lz4','tar-lz4','wgpu','fast',8,'intel'),
             ('nvidia-lz4','tar-lz4','wgpu','fast',8,'nvidia'),
             ('auto-lz4','tar-lz4','auto','fast',8,None),
             ('intel-lz4-4k','tar-lz4','wgpu','fast',8,'intel'),
             ('nvidia-lz4-4k','tar-lz4','wgpu','fast',8,'nvidia')]
    output=(ROOT/args.output).resolve()
    assert output.is_relative_to(ROOT)
    scratch=ROOT/'bench-output'/('portable-'+str(time.time_ns()))
    scratch.mkdir()
    def run(command):
        start=time.perf_counter()
        result=subprocess.run([str(exe),*map(str,command)],capture_output=True,text=True,timeout=180)
        wall=time.perf_counter()-start
        if result.returncode: raise RuntimeError(result.stderr)
        return json.loads(result.stdout),wall
    for dataset in manifest['Datasets']:
        for file in dataset['Files']:
            assert sha(corpus/file['Path'])==file['SHA256'],file['Path']
    report={'executable_sha256':sha(exe),'adapters':run(['gpu-list'])[0],
            'runs':args.runs,'chunk_kib':'16, except GPU -4k configurations use 4','batch_mib':16,
            'method':'Randomized sequential fresh processes; OS cache warm/unspecified, GPU initialization included; host-to-device and device-to-host timestamps exclude CPU staging, which is included in wall time. CPU extraction with byte hash verification every run.',
            'rows':[]}
    work=[(dataset,config,iteration) for dataset in manifest['Datasets'] for config in configs for iteration in range(args.runs)]
    random.Random(4070).shuffle(work)
    try:
        for index,(dataset,config,iteration) in enumerate(work):
            name,fmt,backend,preset,threads,adapter=config
            archive=scratch/'archive'; extracted=scratch/'extracted'
            command=['create',corpus/dataset['Name'],'-o',archive,'-f',fmt,'--backend',backend,'--preset',preset,'--threads',threads,'--batch-mib',16]
            if fmt=='tar-lz4':command+=['--chunk-kib',4 if name.endswith('-4k') else 16]
            if adapter:command+=['--adapter',adapter]
            stats,create_wall=run(command)
            unpack,extract_wall=run(['extract',archive,'-d',extracted])
            for file in dataset['Files']:
                assert sha(extracted/file['Path'])==file['SHA256'],file['Path']
            assert unpack['files']==len(dataset['Files'])
            row={'dataset':dataset['Name'],'config':name,'run':iteration+1,
                 'create_process_seconds':create_wall,'extract_process_seconds':extract_wall,
                 'ratio':stats['output_bytes']/stats['input_bytes'],'stats':stats,'extraction':unpack,'sha256_verified':True}
            report['rows'].append(row)
            output.write_text(json.dumps(report,indent=2))
            print(f"{index+1}/{len(work)} {dataset['Name']} {name}: {create_wall:.3f}s, {row['ratio']:.3%}",flush=True)
            archive.unlink()
            assert extracted.resolve().is_relative_to(scratch.resolve())
            shutil.rmtree(extracted)
    finally:
        output.write_text(json.dumps(report,indent=2))
    scratch.rmdir()

if __name__=='__main__':main()
