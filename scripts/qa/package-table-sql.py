"""Package a verified table-query client while retaining the accepted Cloud/import payload."""
import argparse, hashlib, json, pathlib, zipfile
p=argparse.ArgumentParser();p.add_argument('--candidate',type=pathlib.Path,required=True);p.add_argument('--base',type=pathlib.Path,required=True);a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];candidate=a.candidate.resolve()
info=json.loads((candidate/'build-info.json').read_text(encoding='utf-8-sig'))
sha=lambda data:hashlib.sha256(data).hexdigest().upper()
assert info['status']=='passed' and sha((candidate/'tomato.exe').read_bytes())==info['client']['sha256']
out=candidate/'tomato-portable.zip';assert not out.exists()
overlay={'tomato.exe':candidate/'tomato.exe','build-info.json':candidate/'build-info.json','SHA256SUMS.txt':candidate/'SHA256SUMS.txt','table-query.md':root/'docs/table-query.md'}
for name in ['table-query.md','migration.md','performance.md']:overlay['notices/docs/'+name]=root/'docs'/name
for path in (candidate/'validation').iterdir():
 if path.is_file():overlay['validation/table-sql/'+path.name]=path
with zipfile.ZipFile(a.base) as base,zipfile.ZipFile(out,'x',compression=zipfile.ZIP_DEFLATED,compresslevel=6) as archive:
 for member in base.infolist():
  name=pathlib.PurePosixPath(member.filename);assert not name.is_absolute() and '..' not in name.parts
  if member.filename not in overlay:archive.writestr(member,base.read(member))
 for name,path in overlay.items():archive.write(path,name)
with zipfile.ZipFile(out) as archive,zipfile.ZipFile(a.base) as base:
 assert archive.testzip() is None
 assert len(archive.namelist())==len(set(archive.namelist()))
 assert sha(archive.read('tomato.exe'))==info['client']['sha256']
 assert sha(archive.read('cloud/tomato-cloud.exe'))==info['server']['sha256']
 for name in base.namelist():
  if name not in overlay:assert archive.read(name)==base.read(name),name
result={'status':'passed','archive':str(out),'sha256':sha(out.read_bytes()),'bytes':out.stat().st_size,'retainedCloudAndImportPayload':True,'legacyConfigurationPackaged':False}
(candidate/'archive-validation.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result))
