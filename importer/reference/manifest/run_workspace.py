import hashlib,json,os,subprocess,tempfile
from pathlib import Path
cases=[
('empty-root','workspace={}',None),
('legacy-conflict',"project={name='old',workspace='..'}\nworkspace={}",'WorkspaceConflict'),
('lint-before-optional',"workspace={lints={rust={'rust::bad'='deny'}},dependencies={a={optional=true}}}",'LintName'),
('sorted-public-first',"workspace.dependencies={z={optional=true},a={public=true}}",'WorkspacePublic'),
('optional-before-public',"workspace.dependencies.a={optional=true,public=true}",'WorkspaceOptional'),
('delayed-extras',"workspace.dependencies={a={extra=1},z={optional=true}}",'WorkspaceOptional'),
('false-flags-and-extras',"workspace.dependencies={a={version='1',optional=false,public=false,z=1,a=2}}",None),
('unknown-tool-skips-name',"workspace.lints.other={'rust::bad'='deny'}",None),
('all-warning-branches',"workspace.lints={zzz={'rust::ignored'='deny'},rust={unexpected_cfgs={level='warn',z=1,check-cfg=['cfg(test)'],a=2}},cargo={foo='warn'}}",None),
]
for tool in ['cargo','clippy','rust','rustdoc']:
 for name in ['::','rust::foo','clippy::foo','other::foo::bar']:
  cases.append((f'name-{tool}-{name}',f"workspace.lints.{tool}={{'{name}'='deny'}}",'LintName'))
version=subprocess.run(['cargo','+nightly-2026-07-11','--version','--verbose'],capture_output=True,text=True,check=True).stdout
assert 'commit-hash: 59800466c5c41c444d264b1010b4d57e85a7117f' in version
fixture=tempfile.TemporaryDirectory(prefix='rawr-p9-workspace-corpus-')
root=Path(fixture.name)
rows=[]
for i,(name,source,expected) in enumerate(cases):
 path=root/str(i);path.mkdir();(path/'Cargo.toml').write_text(source+'\n')
 p=subprocess.run(['cargo','+nightly-2026-07-11','metadata','--format-version=1','--no-deps','--offline','--manifest-path',str(path/'Cargo.toml')],capture_output=True,text=True)
 assert (p.returncode==0)==(expected is None),(name,p.stderr)
 if expected:
  fragment={'WorkspaceConflict':'cannot configure both','LintName':'valid lint name','WorkspaceOptional':'workspace dependencies cannot be optional','WorkspacePublic':'workspace dependencies cannot be public'}[expected]
  assert fragment in p.stderr,(name,p.stderr)
 rows.append(dict(name=name,source=source,tag=expected,returncode=p.returncode,stderr=p.stderr.replace(str(root),'<fixture-root>')))
output=Path('importer/reference/manifest/workspace-observations.jsonl')
output.write_text(''.join(json.dumps(row,ensure_ascii=False)+'\n' for row in rows))
print(json.dumps(dict(cases=len(rows),accepted=sum(row['returncode']==0 for row in rows),sha256=hashlib.sha256(output.read_bytes()).hexdigest())))
