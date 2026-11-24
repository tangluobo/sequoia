"""Overlay an accepted import build on the prior Cloud package without dropping its payload."""
import argparse,hashlib,json,pathlib,zipfile
p=argparse.ArgumentParser();p.add_argument('--candidate',required=True,type=pathlib.Path);p.add_argument('--base',required=True,type=pathlib.Path);a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];candidate=a.candidate.resolve();info=json.loads((candidate/'build-info.json').read_text(encoding='utf-8-sig'))
assert info['status']=='passed' and info['client']['sha256']==hashlib.sha256((candidate/'tomato.exe').read_bytes()).hexdigest().upper()
out=candidate/'tomato-import-portable.zip';assert not out.exists()
overlays={'tomato.exe':candidate/'tomato.exe','build-info.json':candidate/'build-info.json','SHA256SUMS.txt':candidate/'SHA256SUMS.txt','data-import.md':root/'docs/data-import.md','data-export.md':root/'docs/data-export.md'}
for name in ['docs/data-import.md','docs/migration.md','docs/performance.md','docs/third-party/access-export-NOTICES.txt','docs/third-party/data-import-NOTICES.txt','vendor/access-export/TomatoAccess.java','vendor/access-export/README.md','vendor/access-export/bin/x86_64-pc-windows-msvc/build-info.json','vendor/odbc-import/odbc.c','vendor/odbc-import/README.md','vendor/odbc-import/bin/i686-pc-windows-msvc/build-info.json','scripts/build-access-export.py','scripts/build-odbc32.py']:
 overlays['notices/'+name]=root/name
# Update the prior package's older build-script alias as well.
overlays['notices/build-access-export.py']=root/'scripts/build-access-export.py'
overlays['NOTICES-access-export.txt']=root/'docs/third-party/access-export-NOTICES.txt'
overlays['NOTICES-data-import.txt']=root/'docs/third-party/data-import-NOTICES.txt'
overlays['notices/README.md']=root/'README.md'
for path in (candidate/'validation').glob('*.json'):
 overlays['validation/import/'+path.name]=path
with zipfile.ZipFile(a.base) as previous,zipfile.ZipFile(out,'x',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as archive:
 for member in previous.infolist():
  path=pathlib.PurePosixPath(member.filename);assert not path.is_absolute() and '..' not in path.parts
  if member.filename not in overlays:archive.writestr(member,previous.read(member))
 for name,path in overlays.items():archive.write(path,name)
with zipfile.ZipFile(out) as archive:
 assert archive.testzip() is None
 assert len(archive.namelist())==len(set(archive.namelist()))
 assert hashlib.sha256(archive.read('tomato.exe')).hexdigest().upper()==info['client']['sha256']
 with zipfile.ZipFile(a.base) as previous:
  for member in previous.namelist():
   if member not in overlays:assert archive.read(member)==previous.read(member),member
result={'status':'passed','archive':str(out),'sha256':hashlib.sha256(out.read_bytes()).hexdigest().upper(),'bytes':out.stat().st_size,'retainedCloudPayload':True,'legacyConfigurationPackaged':False}
(candidate/'archive-validation.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result))
