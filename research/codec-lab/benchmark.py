"""Isolated, warm-cache payload-codec comparison; no production codec defaults change.

Every method receives the same deterministic TAR bytes. Prefixes are capped per
dataset, declared in the manifest. Measures separate creation/extraction process
time, codec time when exposed, exact output bytes, and recovered SHA-256.
"""
import argparse
import hashlib
import io
import json
import lzma
import os
from pathlib import Path
import platform
import statistics
import subprocess
import sys
import tarfile
import time

ROOT=Path(__file__).resolve().parents[2]

def digest(data):return hashlib.sha256(data).hexdigest()

def worker(args):
    data=Path(args[1]).read_bytes()
    started=time.perf_counter()
    if args[0]=='encode':
        result=lzma.compress(data,format=lzma.FORMAT_XZ,check=lzma.CHECK_CRC64,
            filters=[{'id':lzma.FILTER_LZMA2,'preset':6,'dict_size':32*1024*1024}])
    else:result=lzma.decompress(data,format=lzma.FORMAT_XZ,memlimit=128*1024*1024)
    elapsed=(time.perf_counter()-started)*1000
    with open(args[2],'xb') as output:output.write(result);output.flush();os.fsync(output.fileno())
    print(json.dumps({'codec_ms':elapsed,'output_bytes':len(result),'choice':'liblzma-LZMA2-preset6-32MiB'}))

def run(command,cwd):
    start=time.perf_counter()
    result=subprocess.run([str(p) for p in command],cwd=cwd,capture_output=True,timeout=180,
        creationflags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0)
    elapsed=(time.perf_counter()-start)*1000
    if result.returncode:raise RuntimeError(result.stderr.decode(errors='replace')+result.stdout.decode(errors='replace'))
    try:stats=json.loads(result.stdout)
    except ValueError:stats={}
    return elapsed,stats

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repeats',type=int,default=3)
    parser.add_argument('--mib',type=int,default=16)
    parser.add_argument('--datasets',nargs='*')
    parser.add_argument('--rar',type=Path,default=Path('C:/Program Files/WinRAR/Rar.exe'))
    args=parser.parse_args()
    assert 1<=args.repeats<=10 and 1<=args.mib<=128
    binary=ROOT/'target/release/examples/codec_lab.exe'
    assert binary.is_file()
    corpus=ROOT/'bench-output/auto-corpus'
    saved=json.loads((corpus/'manifest.json').read_text(encoding='utf-8-sig'))
    work=ROOT/'bench-output'/('codec-lab-'+str(time.time_ns()));work.mkdir()
    methods=[('adaptive-1','adaptive',1),('adaptive-8','adaptive',8),('zstd1-8','zstd-1',8),
             ('zstd3-1','zstd-3',1),('zstd3-8','zstd-3',8),('zstd9-8','zstd-9',8),('lzma2-6','lzma2',1)]
    if args.rar.is_file():methods += [('rar-m3-8','rar3',8),('rar-m5-8','rar5',8)]
    report={'scope':'Research payload codecs over identical canonical TAR bytes; not a production-format comparison',
        'platform':platform.platform(),'processor':platform.processor(),'python':sys.version,
        'repeats':args.repeats,'dataset_content_cap_bytes':args.mib*1024*1024,
        'cache':'warm/uncontrolled; rotated method order per repetition','peak_memory_measured':False,
        'gpu_execution':False,'binary_sha256':digest(binary.read_bytes()),
        'codec_source_sha256':digest((ROOT/'research/codec-lab/codec.rs').read_bytes()),
        'rar_sha256':digest(args.rar.read_bytes()) if args.rar.is_file() else None,
        'datasets':[],'results':[]}
    for dataset in saved['Datasets']:
        name=dataset['Name']
        if args.datasets and name not in args.datasets:continue
        budget=args.mib*1024*1024;members=[]
        target=work/(name+'.tar')
        with tarfile.open(target,'w',format=tarfile.USTAR_FORMAT) as archive:
            for entry in dataset['Files']:
                if budget<=0:break
                source=corpus/entry['Path'];contents=source.read_bytes()
                assert digest(contents).lower()==entry['SHA256'].lower()
                used=contents[:budget];budget-=len(used)
                info=tarfile.TarInfo(source.name);info.size=len(used);info.mode=0o644;info.mtime=0
                archive.addfile(info,io.BytesIO(used))
                members.append({'name':source.name,'original_bytes':len(contents),'used_bytes':len(used),'used_sha256':digest(used)})
        original=target.read_bytes();expected=digest(original)
        report['datasets'].append({'name':name,'input_bytes':len(original),'sha256':expected,'members':members})
        for repetition in range(args.repeats):
            order=methods[repetition:]+methods[:repetition]
            for label,method,threads in order:
                output=work/f'{name}-{label}-{repetition}.bin';restored=work/f'{name}-{label}-{repetition}.restored'
                if method.startswith('rar'):
                    output=output.with_suffix('.rar')
                    create=[args.rar,'a','-cfg-','-idq','-y','-ma5','-md32m','-m'+method[-1],'-mt'+str(threads),'-s','-ep1',output,target]
                    extract_dir=work/f'extract-{name}-{label}-{repetition}';extract_dir.mkdir()
                    extract=[args.rar,'x','-cfg-','-idq','-y',output,str(extract_dir)+os.sep]
                    restored=extract_dir/target.name
                elif method=='lzma2':
                    create=[sys.executable,Path(__file__),'--worker','encode',target,output]
                    extract=[sys.executable,Path(__file__),'--worker','decode',output,restored]
                else:
                    create=[binary,'encode',method,target,output,threads]
                    extract=[binary,'decode',method,output,restored,threads]
                create_ms,create_stats=run(create,work)
                extract_ms,extract_stats=run(extract,work)
                assert digest(restored.read_bytes())==expected,(name,label)
                row={'dataset':name,'method':label,'repeat':repetition,'input_bytes':len(original),'output_bytes':output.stat().st_size,
                    'create_ms':create_ms,'extract_ms':extract_ms,'encode':create_stats,'decode':extract_stats,'sha256_verified':True,
                    'create_arguments':[str(a) for a in create],'extract_arguments':[str(a) for a in extract]}
                report['results'].append(row)
                (work/'results.json').write_text(json.dumps(report,indent=2),encoding='utf-8')
                print(json.dumps({k:row[k] for k in ['dataset','method','repeat','output_bytes','create_ms']}),flush=True)
                for generated in [output,restored]:
                    assert generated.resolve().is_relative_to(work.resolve());generated.unlink()
                if method.startswith('rar'):extract_dir.rmdir()
        print('Completed '+name,flush=True)
    report['completed']=True
    destination=ROOT/'research/codec-lab/results.json'
    destination.write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    (work/'results.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8')
    print('Results: '+str(destination),flush=True)

if __name__=='__main__':
    if sys.argv[1:2]==['--worker']:worker(sys.argv[2:])
    else:main()
