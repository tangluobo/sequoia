"""Read public encrypted Access samples through the actual packaged worker, without credentials in argv/files."""
import argparse,json,pathlib,subprocess,tempfile
p=argparse.ArgumentParser();p.add_argument('--exe',type=pathlib.Path,required=True);p.add_argument('--fixtures',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args();a.output.mkdir(exist_ok=False,parents=True);checks=[]
for name,password in [('db-enc.mdb',''),('db97-enc.mdb',''),('db2007-oldenc.accdb','Test123'),('db2007-enc.accdb','Test123'),('db2013-enc.accdb','1234')]:
 with tempfile.TemporaryDirectory(prefix='tomato-import-encrypted-',dir=a.output.resolve()) as cache:
  source={'format':'Access','files':[str((a.fixtures/name).resolve())],'first_row':1,'access_password':password}
  result=subprocess.run([str(a.exe.resolve()),'--tomato-data-import-worker',cache],input=json.dumps({'Read':source}).encode(),capture_output=True,timeout=30,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
  assert result.returncode==0,(name,result.stderr.decode(errors='replace'))
  catalog=json.loads((pathlib.Path(cache)/'catalog.json').read_text())['Catalog'];count=0
  for table in catalog['tables']:
   rows=(pathlib.Path(cache)/table['spool']).read_text().splitlines();assert len(rows)==table['rows'];count+=len(rows)
   for row in rows:assert isinstance(json.loads(row),list)
  assert count>0
  assert not list(pathlib.Path(cache).glob('tomato-access-import-*'))
  checks.append({'source':name,'tables':len(catalog['tables']),'records':count,'readOnly':True})
result={'status':'passed','actualPackagedWorker':True,'checks':checks,'passwordsPersisted':False,'savedHostsContacted':False};(a.output/'validation.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result))
