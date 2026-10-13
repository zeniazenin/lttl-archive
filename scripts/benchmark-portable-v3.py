"""Verified transfer-compaction and pipeline ablations on the retained corpus."""
import hashlib, json, os, random, shutil, subprocess, time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
EXE=ROOT/'target/release/lttl.exe'
OLD=ROOT/'target/release/lttl-portable-v2.exe'
CORPUS=ROOT/'bench-output/auto-corpus'
OUTPUT=ROOT/'docs/portable-v3-results.json'
def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest().upper()
def run(exe,args,mode=None):
    env=os.environ.copy();env.pop('LTTL_PORTABLE_TRANSFER',None)
    if mode:env['LTTL_PORTABLE_TRANSFER']=mode
    start=time.perf_counter()
    result=subprocess.run([str(exe),*map(str,args)],env=env,capture_output=True,text=True,timeout=240)
    if result.returncode:raise RuntimeError(result.stderr)
    return json.loads(result.stdout),time.perf_counter()-start

def main():
    manifest=json.loads((CORPUS/'manifest.json').read_text(encoding='utf-8-sig'))
    for data in manifest['Datasets']:
        for file in data['Files']:assert sha(CORPUS/file['Path'])==file['SHA256']
    scratch=ROOT/'bench-output'/('portable-v3-'+str(time.time_ns()));scratch.mkdir()
    configs=[('cpu-lz4-1','cpu','tar-lz4',1,None,None),('cpu-lz4-8','cpu','tar-lz4',8,None,None),('cpu-zstd-fast','cpu','tar-zstd',8,None,None)]
    for vendor in ['intel','nvidia']:
        configs.append((vendor+'-v2','wgpu','tar-lz4',8,vendor,None))
        for mode in ['compact-pipeline','compact-serial','fixed-pipeline','fixed-serial']:
            configs.append((vendor+'-'+mode,'wgpu','tar-lz4',8,vendor,mode))
    work=[(d,c) for d in manifest['Datasets'] for c in configs];random.Random(40703).shuffle(work)
    report={'v3_sha256':sha(EXE),'v2_sha256':sha(OLD),'settings':{'chunk_kib':16,'batch_mib':16,'jobs_per_group':4},
        'method':'Sequential randomized dataset/configuration groups; four jobs per process, first cold and three warm. CPU controls use identical operation boundaries. Timings include transfers, host staging, checksums, framing, file I/O and publication, exclude process startup. Process-group time retained separately. OS/shader caches not flushed. All outputs independently extracted and file SHA256 checked.',
        'rows':[],'groups':[]}
    try:
        for index,(data,config) in enumerate(work):
            name,backend,fmt,threads,adapter,mode=config;job=scratch/str(index);job.mkdir()
            args=['benchmark-session',CORPUS/data['Name'],'--backend',backend,'--preset','fast','--format',fmt,'--threads',threads,'--chunk-kib',16,'--batch-mib',16,'--output-dir',job,'--runs',4]
            if adapter:args+=['--adapter',adapter]
            results,wall=run(OLD if name.endswith('-v2') else EXE,args,mode)
            report['groups'].append({'dataset':data['Name'],'config':name,'process_seconds':wall})
            for result in results:
                archive=Path(result['archive']);assert archive.resolve().is_relative_to(job.resolve())
                dest=job/'unpacked';extraction,extract_wall=run(EXE,['extract',archive,'-d',dest])
                for file in data['Files']:assert sha(dest/file['Path'])==file['SHA256'],file['Path']
                assert extraction['files']==len(data['Files'])
                stats=result['stats'];metrics=stats.get('portable',{})
                if backend=='wgpu':
                    assert metrics['session_reused']==(result['run']>0)
                    if result['run']>0:assert metrics['initialize_seconds']==0
                    if mode:
                        assert metrics['transfer_mode']==mode
                        assert metrics['max_in_flight']==min(metrics['batches'],2 if mode.endswith('pipeline') else 1)
                        assert metrics['d2h_bytes']==metrics['payload_readback_bytes']+metrics['metadata_readback_bytes']
                report['rows'].append({'dataset':data['Name'],'config':name,'run':result['run'],'stats':stats,'extraction':extraction,
                    'extract_process_seconds':extract_wall,'ratio':stats['output_bytes']/stats['input_bytes'],'archive_sha256':sha(archive),'sha256_verified':True})
                archive.unlink();assert dest.resolve().is_relative_to(scratch.resolve());shutil.rmtree(dest)
            job.rmdir();OUTPUT.write_text(json.dumps(report,indent=2),encoding='utf-8')
            print(f'{index+1}/{len(work)} {data["Name"]} {name}: '+', '.join(f'{r["stats"]["elapsed_seconds"]:.3f}s' for r in results),flush=True)
        for data in manifest['Datasets']:
            hashes={r['archive_sha256'] for r in report['rows'] if r['dataset']==data['Name'] and r['config'].startswith(('intel-','nvidia-'))}
            assert len(hashes)==1,(data['Name'],hashes)
        report['gpu_archives_bit_identical_to_v2']=True
    finally:OUTPUT.write_text(json.dumps(report,indent=2),encoding='utf-8')
    scratch.rmdir()
if __name__=='__main__':main()
