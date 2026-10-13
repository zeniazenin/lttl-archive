"""Publish the user-requested repository release using existing Git credentials.

Credentials are held only in memory and sent only to GitHub API endpoints.
"""
import argparse
import hashlib
import json
import os
import re
import tomllib
from pathlib import Path
import subprocess
import urllib.request
import urllib.error
import urllib.parse

ROOT=Path(__file__).resolve().parents[1]
REPO='zeniazenin/lttl-archive'

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--tag',required=True)
    parser.add_argument('--publish',action='store_true')
    args=parser.parse_args()
    if not re.fullmatch(r'v[0-9]+\.[0-9]+\.[0-9]+(?:-[A-Za-z0-9.-]+)?',args.tag): raise SystemExit('Invalid release tag')
    version=args.tag[1:]
    if version != tomllib.loads((ROOT/'Cargo.toml').read_text())['package']['version']: raise SystemExit('Tag does not match package version')
    env=dict(os.environ,GIT_TERMINAL_PROMPT='0',GCM_INTERACTIVE='never')
    result=subprocess.run(['git','credential','fill'],input='protocol=https\nhost=github.com\npath='+REPO+'.git\n\n',text=True,capture_output=True,cwd=ROOT,env=env)
    if result.returncode:raise SystemExit('Git credential lookup failed (credential output withheld)')
    credential=dict(line.split('=',1) for line in result.stdout.splitlines() if '=' in line)
    token=credential.get('password')
    if not token:raise SystemExit('No existing GitHub credential found')
    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self,*args,**kwargs):return None
    opener=urllib.request.build_opener(NoRedirect)
    def api(method,path,data=None,blob=None):
        url=path if path.startswith('https://') else 'https://api.github.com/repos/'+REPO+path
        parts=urllib.parse.urlsplit(url)
        if parts.scheme!='https' or parts.hostname not in ['api.github.com','uploads.github.com']:
            raise SystemExit('Refusing credentials to unexpected endpoint')
        headers={'Authorization':'Bearer '+token,'Accept':'application/vnd.github+json','X-GitHub-Api-Version':'2022-11-28','User-Agent':'lttl-release-packaging'}
        payload=None
        if data is not None:payload=json.dumps(data).encode();headers['Content-Type']='application/json'
        if blob is not None:payload=blob;headers['Content-Type']='application/octet-stream'
        try:
            with opener.open(urllib.request.Request(url,data=payload,headers=headers,method=method),timeout=240) as response:
                body=response.read()
                return json.loads(body) if body else None
        except urllib.error.HTTPError as exc:
            raise SystemExit(f'GitHub API {method} failed with HTTP {exc.code}; response withheld') from None
    repo=api('GET','')
    print(json.dumps({k:repo[k] for k in ['full_name','private','html_url']}|{'can_push':repo.get('permissions',{}).get('push')}))
    if not args.publish:return
    if subprocess.check_output(['git','status','--porcelain'],cwd=ROOT,text=True).strip():
        raise SystemExit('Commit the complete release before publishing')
    setup=ROOT/'dist'/f'lttl-{version}-windows-x64-setup.exe'
    report=json.loads((ROOT/'docs/windows-installer-validation.json').read_text())
    if report['installer']!=setup.name or report['sha256']!=hashlib.sha256(setup.read_bytes()).hexdigest():
        raise SystemExit('Installer does not match the committed validation report')
    if not any('Installed client passes' in check for check in report['checks']):
        raise SystemExit('Installed audit/retention integration must pass before publishing')
    commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    remote=api('GET','/git/ref/heads/main')['object']['sha']
    if remote!=commit:raise SystemExit('Remote main does not match the tested commit')
    # An existing tag must already point at the intended commit. Never move a tag.
    tags=api('GET','/git/matching-refs/tags/'+args.tag)
    tagged=next((t for t in tags if t['ref']=='refs/tags/'+args.tag),None)
    if tagged and (tagged['object']['type']!='commit' or tagged['object']['sha']!=commit):
        raise SystemExit('Existing tag differs from tested HEAD')
    releases=api('GET','/releases?per_page=100')
    existing=next((r for r in releases if r['tag_name']==args.tag),None)
    if existing and not existing['draft']:raise SystemExit('Release is already published; refusing to replace it')
    if existing and existing['target_commitish']!=commit:raise SystemExit('Existing draft targets another commit; review it before resuming')
    body=(ROOT/'docs/releases'/f'{args.tag}.md').read_text(encoding='utf-8')
    release=existing or api('POST','/releases',{'tag_name':args.tag,'target_commitish':commit,'name':f'lttl {version}','body':body,'draft':True,'prerelease':True})
    names=[setup.name,setup.name+'.sha256',f'lttl-{version}-payload-sha256.json']
    checksum=(ROOT/'dist'/(setup.name+'.sha256')).read_text().split()[0]
    if checksum!=report['sha256']: raise SystemExit('Installer checksum file mismatch')
    assets={a['name']:a for a in release['assets']}
    for name in names:
        payload=(ROOT/'dist'/name).read_bytes();digest='sha256:'+hashlib.sha256(payload).hexdigest()
        if name in assets:
            asset=assets[name]
        else:
            asset=api('POST',release['upload_url'].split('{')[0]+'?name='+urllib.parse.quote(name),blob=payload)
        if asset['size']!=len(payload) or asset.get('digest')!=digest:
            raise SystemExit('Uploaded asset digest/size mismatch: '+name)
        print(json.dumps({'name':name,'size':asset['size'],'digest':asset['digest']}))
    published=api('PATCH','/releases/'+str(release['id']),{'draft':False,'prerelease':True,'body':body})
    verify=api('GET','/releases/tags/'+args.tag)
    assert not verify['draft'] and len(verify['assets'])==3
    if api('GET','/git/ref/tags/'+args.tag)['object']['sha']!=commit:
        raise SystemExit('Published tag does not match the tested commit')
    print(json.dumps({'release':verify['html_url'],'commit':commit,'assets':[a['browser_download_url'] for a in verify['assets']]}))

if __name__=='__main__':main()
