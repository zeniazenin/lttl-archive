"""Cold/warm GPU jobs, fresh v1 controls, and CPU controls; verify every archive."""
import hashlib, json, random, shutil, subprocess, time
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
EXE=ROOT/'target/release/lttl.exe'
OLD=ROOT/'target/release/lttl-portable-v1.exe'
CORPUS=ROOT/'bench-output/auto-corpus'
OUTPUT=ROOT/'docs/portable-v2-results.json'
def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest().upper()
def run(exe,args):
    start=time.perf_counter()
    result=subprocess.run([str(exe),*map(str,args)],capture_output=True,text=True,timeout=180)
    if result.returncode:raise RuntimeError(result.stderr)
    return json.loads(result.stdout),time.perf_counter()-start

def main():
    manifest=json.loads((CORPUS/'manifest.json').read_text(encoding='utf-8-sig'))
    for data in manifest['Datasets']:
        for file in data['Files']:assert sha(CORPUS/file['Path'])==file['SHA256']
    scratch=ROOT/'bench-output'/('portable-v2-'+str(time.time_ns()));scratch.mkdir()
    configs=[('cpu-lz4-1','cpu','tar-lz4','fast',1,None),('cpu-lz4-8','cpu','tar-lz4','fast',8,None),
             ('cpu-zstd-fast','cpu','tar-zstd','fast',8,None),('cpu-zstd-balanced','cpu','tar-zstd','balanced',8,None),
             ('auto-lz4','auto','tar-lz4','fast',8,None),
             ('intel-v2','wgpu','tar-lz4','fast',8,'intel'),('nvidia-v2','wgpu','tar-lz4','fast',8,'nvidia'),
             ('intel-v2-cold','wgpu','tar-lz4','fast',8,'intel'),('nvidia-v2-cold','wgpu','tar-lz4','fast',8,'nvidia'),
             ('intel-v1','wgpu','tar-lz4','fast',8,'intel'),('nvidia-v1','wgpu','tar-lz4','fast',8,'nvidia')]
    work=[(d,c) for d in manifest['Datasets'] for c in configs];random.Random(40702).shuffle(work)
    report={'v2_sha256':sha(EXE),'v1_sha256':sha(OLD),'settings':{'chunk_kib':16,'batch_mib':16},
            'method':'Sequential randomized jobs. V2 creates four archives in one process: first job has cold context, next three reuse it. -cold creates three jobs while clearing the context before each. V1 uses three fresh processes. CPU controls use four jobs in one process. Extraction timed separately and every file SHA-256 verified. OS/shader caches not flushed.',
            'rows':[],'groups':[]}
    try:
        for index,(data,config) in enumerate(work):
            name,backend,fmt,preset,threads,adapter=config
            job=scratch/str(index);job.mkdir();legacy=name.endswith('-v1');cold=name.endswith('-cold')
            args=[CORPUS/data['Name'],'--backend',backend,'--preset',preset,'--format',fmt,'--threads',threads,'--chunk-kib',16,'--batch-mib',16]
            if adapter:args+=['--adapter',adapter]
            if legacy:
                results=[];process_seconds=0
                for repeat in range(3):
                    archive=job/f'run-{repeat}.tar.lz4'
                    stats,wall=run(OLD,['create',*args,'-o',archive]);process_seconds+=wall
                    results.append({'run':repeat,'archive':str(archive),'stats':stats,'process_seconds':wall})
            else:
                results,process_seconds=run(EXE,['benchmark-session',*args,'--output-dir',job,'--runs',3 if cold else 4,*(['--cold'] if cold else [])])
            report['groups'].append({'dataset':data['Name'],'config':name,'process_seconds':process_seconds,'job_count':len(results)})
            for result in results:
                archive=Path(result['archive']);assert archive.resolve().is_relative_to(job.resolve())
                destination=job/'unpacked'
                extraction,extract_wall=run(EXE,['extract',archive,'-d',destination])
                for file in data['Files']:assert sha(destination/file['Path'])==file['SHA256'],file['Path']
                assert extraction['files']==len(data['Files'])
                stats=result['stats'];metrics=stats.get('portable',{})
                if backend=='wgpu' and not legacy:
                    assert metrics['session_reused']==(result['run']>0 and not cold)
                    if metrics['session_reused']:assert metrics['initialize_seconds']==0
                report['rows'].append({'dataset':data['Name'],'config':name,'run':result['run'],'stats':stats,
                    'process_seconds':result.get('process_seconds'),'extraction':extraction,'extract_process_seconds':extract_wall,
                    'ratio':stats['output_bytes']/stats['input_bytes'],'archive_sha256':sha(archive),'sha256_verified':True})
                archive.unlink();assert destination.resolve().is_relative_to(scratch.resolve());shutil.rmtree(destination)
            job.rmdir();OUTPUT.write_text(json.dumps(report,indent=2),encoding='utf-8')
            print(f'{index+1}/{len(work)} {data["Name"]} {name}: '+', '.join(f'{r["stats"]["elapsed_seconds"]:.3f}s' for r in results),flush=True)
        # Same wire output, not just the same size: compare old/new and both GPUs.
        for data in manifest['Datasets']:
            hashes={row['archive_sha256'] for row in report['rows'] if row['dataset']==data['Name'] and row['config'].startswith(('intel-','nvidia-'))}
            assert len(hashes)==1,(data['Name'],hashes)
        report['gpu_archives_bit_identical_to_v1']=True
    finally:OUTPUT.write_text(json.dumps(report,indent=2),encoding='utf-8')
    scratch.rmdir()
if __name__=='__main__':main()
