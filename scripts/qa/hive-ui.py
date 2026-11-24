"""Run owned HS2 and actual egui/actor workflows without operating the desktop."""
from pathlib import Path
import argparse,hashlib,json,os,re,shutil,subprocess,time,uuid
ROOT=Path(__file__).resolve().parents[2]
def sha(p):
 with p.open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest().upper()
def run(args):
 assert re.fullmatch(r'[a-z0-9-]+',args.label)
 out=ROOT/'target/native-checks'/args.label;out.mkdir(parents=True,exist_ok=False)
 copied={}
 for name,source in [('services',args.services),('ui',args.ui)]:
  source=source.resolve(strict=True);assert source.is_relative_to((ROOT/'target').resolve())
  def production(folder):
   return [p for p in folder.rglob('*.rs')if 'tests'not in p.relative_to(folder).parts and p.name!='tests.rs'and not p.name.endswith('_tests.rs')]
  inputs=production(ROOT/'crates/core/src')+production(ROOT/'crates/services/src')
  if name=='services':inputs+=[ROOT/'crates/services/src/database/hive/tests.rs']+list((ROOT/'crates/services/src/database/hive/tests').rglob('*.rs'))
  if name=='ui':inputs+=production(ROOT/'crates/ui-egui/src')+[ROOT/'crates/ui-egui/src/app/tests/hive.rs']
  assert all(p.stat().st_mtime<source.stat().st_mtime for p in inputs),'Harness predates sources: '+name
  dest=out/(name+'-harness'+source.suffix);shutil.copy2(source,dest);assert sha(dest)==sha(source);copied[name]=dest
 profile=out/('tomato-hive-ui-fixture-'+uuid.uuid4().hex);profile.mkdir()
 env=os.environ.copy();env.update(TOMATO_HIVE_UI_FIXTURE=str(profile),TOMATO_DATA_DIR=str(profile),TEMP=str(ROOT/'target/build-temp'),TMP=str(ROOT/'target/build-temp'))
 shared=[Path(os.environ.get('USERPROFILE',str(Path.home())))/'.tomato'/n for n in ['connections.json','global.json']]
 before={str(p):sha(p)for p in shared if p.exists()}
 flags=0x08000000 if os.name=='nt' else 0
 with (out/'server.log').open('w',encoding='utf-8')as log:
  host=subprocess.Popen([str(copied['services']),'--ignored','--exact','database::hive::tests::server::ui::host_hive_ui_fixture','--nocapture'],cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT,creationflags=flags)
  try:
   deadline=time.monotonic()+35
   while not (profile/'ready').exists():
    assert host.poll()is None,'Owned HS2 exited before readiness'
    assert time.monotonic()<deadline,'Owned HS2 did not become ready'
    time.sleep(.05)
   with (out/'ui.log').open('w',encoding='utf-8')as ui_log:
    result=subprocess.run([str(copied['ui']),'--ignored','--exact','app::tests::hive::actual_hive_lazy_tree_design_preview_save_sibling_refresh_query_and_disconnect','--nocapture'],cwd=ROOT,env=env,stdout=ui_log,stderr=subprocess.STDOUT,creationflags=flags,timeout=120)
   assert result.returncode==0,'Native Hive control/actor workflow failed; inspect ui.log and server.log'
   (profile/'stop').write_text('stop')
   assert host.wait(timeout=25)==0,'Owned HS2 failed'
   assert before=={str(p):sha(p)for p in shared if p.exists()},'Shared configuration changed externally during the isolated check'
   server=json.loads((profile/'server-validation.json').read_text(encoding='utf-8'))
   assert server['passed']and server['ddlExecutedOnce']and server['closedSessions']>=2
   proof={'passed':True,'harnesses':{k:{'path':str(p.relative_to(ROOT)),'sha256':sha(p)}for k,p in copied.items()},'sourceFreshnessScope':'production and Hive workflow fixture sources; complete default suites are checked separately','server':server,'sharedConfigurationUnchanged':True,'savedHostsContacted':False,'systemClipboardWritten':False,'actualNativeControls':True,'operatingSystemInputUsed':False}
   (out/'validation.json').write_text(json.dumps(proof,indent=2)+'\n',encoding='utf-8');print(json.dumps({'passed':True,'output':str(out),'server':server}))
  finally:
   (profile/'stop').write_text('stop')
   if host.poll()is None:
    try:host.wait(timeout=10)
    except subprocess.TimeoutExpired:host.kill();host.wait()
if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('--label',required=True);parser.add_argument('--services',type=Path,required=True);parser.add_argument('--ui',type=Path,required=True);run(parser.parse_args())
