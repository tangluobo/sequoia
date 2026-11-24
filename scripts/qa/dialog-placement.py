"""Capture owned native dialogs and verify physical work-area centering without OS input."""
from pathlib import Path
import argparse,hashlib,json,os,re,shutil,subprocess,time,struct
parser=argparse.ArgumentParser();parser.add_argument('--executable',type=Path,required=True);parser.add_argument('--label',required=True);parser.add_argument('--harness',action='store_true');args=parser.parse_args()
assert re.fullmatch(r'[a-z0-9-]+',args.label)
r=Path(__file__).resolve().parents[2];d=r/'target/window-probes'/args.label;d.mkdir(parents=True,exist_ok=False)
def sha(p):
 with p.open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest().upper()
source=args.executable.resolve(strict=True);digest=sha(source);exe=d/'tomato.exe';shutil.copy2(source,exe);assert sha(exe)==digest==sha(source)
shared=Path.home()/'.tomato/connections.json';before=sha(shared);cases=[]
for case in ['import','export','backup_error','restore_confirm']:
 profile=d/(('tomato-export-fixture-' if case in ['import','export'] else 'tomato-backup-fixture-')+case);profile.mkdir()
 image=d/(case+'.png');env=os.environ.copy();env.update(TOMATO_DATA_DIR=str(profile),TOMATO_UI_PREVIEW=case,TOMATO_UI_CAPTURE=str(image),TOMATO_UI_CAPTURE_DELAY_MS='2400',TOMATO_UI_CAPTURE_FRAMES='3',TOMATO_EXPORT_SELECTION='partial',WGPU_BACKEND='vulkan',TEMP=str(r/'target/build-temp'),TMP=str(r/'target/build-temp'))
 command=[str(exe)]+(['--ignored','--exact','capture_desktop','--nocapture']if args.harness else [])
 with(d/(case+'.stdout.log')).open('wb')as out,(d/(case+'.stderr.log')).open('wb')as err:
  child=subprocess.Popen(command,cwd=r,env=env,stdout=out,stderr=err,creationflags=0x08000000)
  try:assert child.wait(timeout=90)==0,case+' native process failed'
  finally:
   if child.poll()is None:child.terminate();child.wait(timeout=15)
 assert image.exists(),case+' no native framebuffer'
 geometry=json.loads(image.with_suffix('.geometry.json').read_text(encoding='utf-8'))
 target=[v for v in geometry if v['target']];assert len(target)==1 and not target[0]['root'],case+' wrong viewport'
 v=target[0];a=v['outerPx'];w=v['workPx'];expected=[w[i]+max(w[i+2]-w[i]-(a[i+2]-a[i]),0)/2 for i in [0,1]]
 error=[abs(a[i]-expected[i])for i in [0,1]];assert max(error)<=1.5,(case,v,expected,error)
 width,height=struct.unpack('>II',image.read_bytes()[16:24]);assert width>0 and height>0
 frames=[dict(path=str(p),sha256=sha(p))for p in [image,image.with_name(case+'-01.png'),image.with_name(case+'-02.png')]]
 assert image.with_suffix('.close').is_file()
 cases.append(dict(case=case,physicalGeometry=v,centerErrorPx=error,frames=frames,normalExit=True))
 assert sha(shared)==before
 print('Native placement passed: '+case,flush=True)
assert sha(source)==digest==sha(exe)
proof=dict(passed=True,executableSha256=digest,testHarness=args.harness,cases=cases,sharedConfigurationUnchanged=True,savedHostsContacted=False,physicalOsInputUsed=False,physicalMixedDpiMonitorsTested=False,javaPixelParityClaimed=False)
(d/'validation.json').write_text(json.dumps(proof,indent=2),encoding='utf-8');print(str(d/'validation.json'),flush=True)
