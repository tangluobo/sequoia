"""Read the owned destination databases after native wizard exit, without Tomato's readers."""
import argparse,json,pathlib,sqlite3
p=argparse.ArgumentParser();p.add_argument('directory',type=pathlib.Path);a=p.parse_args()
manifest=json.loads((a.directory/'validation.json').read_text(encoding='utf-8-sig'))
assert manifest['status']=='passed'
proof=[]
for case in manifest['checks']:
 if not case['case'].startswith('run-'):continue
 state=case['state'];db=pathlib.Path(state['database'])
 assert db.is_file() and db.parent.name.startswith('tomato-import-fixture-')
 with sqlite3.connect(f'{db.as_uri()}?mode=ro',uri=True) as conn:
  summary=conn.execute('SELECT COUNT(*),COUNT(DISTINCT id),MIN(id),MAX(id),SUM(id) FROM records').fetchone()
  assert summary==(2305,2305,1,2305,2305*2306//2),(case['case'],summary)
  rows=conn.execute('SELECT id,note,postal FROM records ORDER BY id').fetchall()
  for number,note,postal in rows:
   assert note==f'第{number}行',(case['case'],number,repr(note))
   assert postal=='00123',(case['case'],number,repr(postal))
 proof.append({'case':case['case'],'database':str(db),'records':len(rows),'everyRecordReadBack':True,'unicodeAndLeadingZeroes':True})
assert proof
out={'status':'passed','guiSha256':manifest['guiSha256'],'independentReader':'Python sqlite3','checks':proof,'savedHostsContacted':False}
(a.directory/'readback.json').write_text(json.dumps(out,ensure_ascii=False,indent=2),encoding='utf-8')
print(json.dumps({'status':'passed','cases':len(proof),'records':sum(x['records'] for x in proof)}))
