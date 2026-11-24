"""Capture an owned release Hive designer window; never use saved connections."""
from pathlib import Path
import argparse,hashlib,json,os,re,shutil,subprocess,time,uuid
ROOT=Path(__file__).resolve().parents[2]
def sha(p):
 with p.open('rb')as f:return hashlib.file_digest(f,'sha256').hexdigest().upper()
def run(args):
 assert re.fullmatch(r'[a-z0-9-]+',args.label)
 out=ROOT/'target/window-probes'/args.label;out.mkdir(parents=True,exist_ok=False)
 for p in [args.program,args.services]:assert p.resolve(strict=True).is_relative_to((ROOT/'target').resolve())
 gui=out/'tomato.exe';host_program=out/'services-harness.exe';shutil.copy2(args.program,gui);shutil.copy2(args.services,host_program)
 assert sha(gui)==sha(args.program)and sha(host_program)==sha(args.services)
 profile=out/('tomato-hive-ui-fixture-'+uuid.uuid4().hex);profile.mkdir()
 env=os.environ.copy();env.update(TOMATO_HIVE_UI_FIXTURE=str(profile),TOMATO_HIVE_UI_CAPTURE='owned-fixture',TOMATO_DATA_DIR=str(profile),TOMATO_UI_PREVIEW='hive_designer',TOMATO_UI_CAPTURE=str(out/'hive.png'),TOMATO_UI_CAPTURE_FRAMES='4',TOMATO_UI_CAPTURE_DELAY_MS='2500',WGPU_BACKEND='vulkan',TEMP=str(ROOT/'target/build-temp'),TMP=str(ROOT/'target/build-temp'))
 shared=Path(os.environ.get('USERPROFILE',str(Path.home())))/'.tomato/connections.json';before=sha(shared)if shared.exists()else None
 flags=0x08000000 if os.name=='nt'else 0
 with (out/'server.log').open('w',encoding='utf-8')as server_log:
  host=subprocess.Popen([str(host_program),'--ignored','--exact','database::hive::tests::server::ui::host_hive_ui_fixture','--nocapture'],cwd=ROOT,env=env,stdout=server_log,stderr=subprocess.STDOUT,creationflags=flags)
  try:
   deadline=time.monotonic()+45
   while not(profile/'ready').exists():
    assert host.poll()is None and time.monotonic()<deadline,'Owned HS2 did not become ready';time.sleep(.05)
   with(out/'gui.log').open('w',encoding='utf-8')as gui_log:
    result=subprocess.run([str(gui)],cwd=gui.parent,env=env,stdout=gui_log,stderr=subprocess.STDOUT,creationflags=flags,timeout=65)
   assert result.returncode==0,'Native release GUI failed'
   image=out/'hive-03.png';assert image.is_file(),'Missing native framebuffer'
   (profile/'stop').write_text('stop');assert host.wait(timeout=20)==0
   server=json.loads((profile/'server-validation.json').read_text(encoding='utf-8'));assert server['passed']and server['closedSessions']>0
   assert before==(sha(shared)if shared.exists()else None),'Shared configuration changed externally'
   report={'passed':True,'guiSha256':sha(gui),'image':str(image.relative_to(ROOT)),'imageSha256':sha(image),'normalExit':True,'server':server,'sharedConfigurationUnchanged':True,'savedHostsContacted':False,'systemClipboardWritten':False,'fullJavaVisualParityClaimed':False}
   (out/'validation.json').write_text(json.dumps(report,indent=2)+'\n',encoding='utf-8');print(json.dumps(report))
  finally:
   (profile/'stop').write_text('stop')
   if host.poll()is None:
    try:host.wait(timeout=10)
    except subprocess.TimeoutExpired:host.kill();host.wait()
if __name__=='__main__':
 parser=argparse.ArgumentParser();parser.add_argument('--label',required=True);parser.add_argument('--program',type=Path,required=True);parser.add_argument('--services',type=Path,required=True);run(parser.parse_args())
