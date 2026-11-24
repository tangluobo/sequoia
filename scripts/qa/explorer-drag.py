"""Human Windows Explorer drop acceptance using a signed, isolated loopback S3 fixture.
No desktop input, clipboard writes, saved hosts or user configuration writes.
"""
from pathlib import Path
import argparse,hashlib,json,os,shutil,subprocess,threading,time,uuid
from http.server import BaseHTTPRequestHandler,ThreadingHTTPServer
from urllib.parse import urlsplit,unquote,parse_qs
from xml.sax.saxutils import escape

parser=argparse.ArgumentParser()
parser.add_argument('--executable',required=True,type=Path)
parser.add_argument('--label',required=True)
parser.add_argument('--matrix',action='store_true')
parser.add_argument('--batch',action='store_true',help='Require one completed drag containing all four files')
parser.add_argument('--progress',action='store_true',help='Stream larger fixtures in committed chunks for native copy-dialog acceptance')
parser.add_argument('--view',choices=['LIST','ICON','COLUMN'],default='LIST')
parser.add_argument('--download-delay',type=float,default=0.)
parser.add_argument('--timeout',type=int,default=900,help='Seconds before closing the owned window; 0 keeps it open until the user closes it or validation succeeds')
args=parser.parse_args()
assert 0. <= args.download_delay <= 5.
assert args.timeout >= 0
root=Path(__file__).resolve().parents[2]
assert args.label.replace('-','').isalnum() and args.label==args.label.lower()
source=args.executable.resolve(); assert source.is_relative_to(root/'target') and source.name=='tomato.exe'
probe=root/'target/window-probes'/args.label;probe.mkdir()
gui=probe/'tomato.exe';shutil.copy2(source,gui)
sha=lambda p:hashlib.file_digest(Path(p).open('rb'),'sha256').hexdigest().upper()
gui_sha=sha(gui);assert gui_sha==sha(source)
runner_sha=sha(Path(__file__))
shutil.copy2(Path(__file__),probe/'runner.py')
profile=probe/('tomato-file-drop-fixture-'+uuid.uuid4().hex);(profile/'source').mkdir(parents=True)
owned_temp=profile/'temp';owned_temp.mkdir()
destination=probe/'Explorer接收目录';destination.mkdir()
names=['1-列表拖出-中文.txt','2-图标拖出-中文.txt','3-列视图拖出-中文.txt','4-空文件.txt'] if args.matrix or args.batch else ['1-列表拖出-中文.txt']
objects={name:((f'Tomato actual Explorer drop: {name}\n原始远端文件字节。\n'*250).encode('utf-8')if not name.startswith('4-')else b'')for name in names}
if args.progress:
 objects={name:((data*((8*1024*1024+len(data)-1)//len(data)))[:8*1024*1024]if data else data)for name,data in objects.items()}
expected_views={name:(args.view if args.batch else('ICON'if name.startswith('2-')else'COLUMN'if name.startswith('3-')else args.view))for name in names}
bucket='tomato-explorer-fixture';requests=[];lock=threading.Lock()
shared=Path(os.environ['USERPROFILE'])/'.tomato/connections.json';before=sha(shared)
proof_path=probe/'browser-state.json'
def write(path,value):
 pending=path.with_suffix(path.suffix+'.pending')
 pending.write_text(json.dumps(value,ensure_ascii=False,indent=2),encoding='utf-8')
 pending.replace(path)
class Handler(BaseHTTPRequestHandler):
 def log_message(self,*_):pass
 def respond(self,status,body=b'',length=None,headers=None,throttle=False):
  self.send_response(status);self.send_header('Content-Length',str(len(body)if length is None else length));self.send_header('ETag','"fixture-etag"');self.send_header('Last-Modified','Wed, 01 Jan 2025 00:00:00 GMT')
  for key,value in (headers or {}).items():self.send_header(key,value)
  self.end_headers()
  if self.command!='HEAD':
   if throttle:
    for offset in range(0,len(body),64*1024):
     self.wfile.write(body[offset:offset+64*1024]);self.wfile.flush()
     if offset+64*1024<len(body):time.sleep(.015)
   else:self.wfile.write(body)
 def serve(self):
  auth=self.headers.get('Authorization','')
  if not auth.startswith('AWS4-HMAC-SHA256 ')or'fixture-key'not in auth:return self.respond(403)
  uri=urlsplit(self.path);parts=unquote(uri.path).lstrip('/').split('/',1);key=parts[1]if len(parts)>1 else'';query=parse_qs(uri.query)
  if not key:
   prefix=query.get('prefix',[''])[0]
   text='<ListBucketResult><Name>'+bucket+'</Name><IsTruncated>false</IsTruncated>'
   for name,data in objects.items():
    if name.startswith(prefix):text+=f'<Contents><Key>{escape(name)}</Key><LastModified>2025-01-01T00:00:00Z</LastModified><Size>{len(data)}</Size><ETag>"fixture-etag"</ETag></Contents>'
   return self.respond(200,(text+'</ListBucketResult>').encode())
  if key not in objects:return self.respond(404)
  data=objects[key];status=200;headers={}
  if self.command=='GET' and args.download_delay:time.sleep(args.download_delay)
  if self.command in ('GET','HEAD'):
   try:view=json.loads(proof_path.read_text(encoding='utf-8'))['view']
   except (OSError,ValueError,KeyError):view=None
   request=dict(name=key,view=view,method=self.command,range=self.headers.get('Range'),startedAt=time.time())
   with lock:requests.append(request)
  if self.headers.get('Range'):
   first,last=self.headers['Range'].removeprefix('bytes=').split('-',1);start=int(first);end=min(int(last)if last else len(data)-1,len(data)-1)
   if start>=len(data):return self.respond(416)
   headers['Content-Range']=f'bytes {start}-{end}/{len(data)}';data=data[start:end+1];status=206
  try:self.respond(status,data,headers=headers,throttle=args.progress)
  finally:
   with lock:request['completedAt']=time.time()
 do_GET=serve
 do_HEAD=serve
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
connection=dict(id=str(uuid.uuid4()),name='文件拖放验收',type='S3',host='127.0.0.1',bucket=bucket,region='us-east-1',username='fixture-key',password='fixture-secret',endpoint=f'http://127.0.0.1:{server.server_port}',pathStyleAccess=True)
config=profile/'connections.json';write(config,[connection]);own_before=sha(config)
env=os.environ.copy();env.update(TEMP=str(owned_temp),TMP=str(owned_temp),TOMATO_DATA_DIR=str(profile),TOMATO_UI_PREVIEW='file_drop',TOMATO_UI_CAPTURE=str(probe/'unused.png'),TOMATO_FILE_DROP_CASE='manual',TOMATO_FILE_DROP_VIEW=args.view,TOMATO_FILE_DROP_TARGET='/',TOMATO_FILE_DROP_PROOF=str(proof_path),WGPU_BACKEND='vulkan')
for key in ['TOMATO_TEST_SAVED_110','TOMATO_RDP_CAPTURE_HOLD','TOMATO_IMAGE_BROWSER_MENU','TOMATO_IMAGE_BROWSER_MARKDOWN']:env.pop(key,None)
stdout=(probe/'stdout.log').open('wb');stderr=(probe/'stderr.log').open('wb')
# Open the receiver first: a late Explorer window previously hid the test application.
subprocess.Popen(['explorer.exe',str(destination)])
time.sleep(1.)
startup=subprocess.STARTUPINFO();startup.dwFlags|=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=1 # SW_SHOWNORMAL
child=subprocess.Popen([str(gui)],cwd=probe,env=env,stdout=stdout,stderr=stderr,startupinfo=startup,creationflags=subprocess.CREATE_NO_WINDOW)
title='Tomato 文件拖出验收 · '+args.label
write(probe/'ready.json',dict(guiPid=child.pid,guiSha256=gui_sha,destination=str(destination),profile=str(profile),downloadDelaySeconds=args.download_delay,timeoutSeconds=args.timeout,windowTitle=title,batch=args.batch,streamingProgress=args.progress,expected=[dict(name=n,bytes=len(b),sha256=hashlib.sha256(b).hexdigest().upper(),view=expected_views[n])for n,b in objects.items()],osInputInjected=False))
print(json.dumps(dict(guiPid=child.pid,destination=str(destination),files=names,windowTitle=title),ensure_ascii=False),flush=True)
print('请从 Tomato 文件列表按住文件名，拖到 Explorer接收目录的空白处后松开。',flush=True)
if args.batch:print('先单击一个文件，再按 Ctrl+A 选中四项，保持多选状态拖动一次。无需切换视图。',flush=True)
print('窗口不会自动过期，关闭 Tomato 即可结束。'if args.timeout==0 else f'本轮将在 {args.timeout} 秒后结束。',flush=True)
deadline=time.monotonic()+args.timeout if args.timeout else float('inf');matched=False;events=[]
last_phase=None;end_reason=None;first_destination_data={};last_native_state=None
def matches(name,data):
 try:return (destination/name).read_bytes()==data
 except OSError:return False
try:
 while time.monotonic()<deadline and child.poll()is None:
  matched=False
  for name in names:
   try:
    if name not in first_destination_data and objects[name]:
     with(destination/name).open('rb')as partial:prefix=partial.read(min(64*1024,len(objects[name])))
     if prefix and prefix==objects[name][:len(prefix)]:first_destination_data[name]=time.time()
   except OSError:pass
  with lock:read_views=all(any(r['name']==name and r['view']==expected_views[name]for r in requests)for name in names)
  try:
   state=json.loads(proof_path.read_text(encoding='utf-8'));events=state.get('exportEvents')or[]
   if state.get('nativeState'):last_native_state=state['nativeState']
   if args.batch:
    native_complete=any(e.get('phase')=='文件拖放完成'and e.get('view')==args.view and set(e.get('files')or[])==set(names)for e in events)
    read_views=native_complete
   else:native_complete=sum(e.get('phase')=='文件拖放完成'for e in events)>=len(names)
  except (OSError,ValueError):native_complete=False
  # Verify full payloads once the Shell reports completion. Polling 8 MiB files
  # while the producer is writing would distort the transfer speed on USB disks.
  matched=native_complete and all(matches(n,data)for n,data in objects.items())
  if matched and read_views and native_complete:
   end_reason='validated'
   break
  matched=False
  # Keep observations durable if the desktop or runner is interrupted before teardown.
  with lock: live_requests=list(requests)
  write(probe/'observations.json',dict(guiPid=child.pid,guiSha256=gui_sha,observedAt=time.time(),signedRequests=live_requests,nativeExportEvents=events,nativeState=last_native_state,firstDestinationDataAt=first_destination_data,destinationFiles=[p.name for p in destination.iterdir() if p.is_file()]))
  current_phase=next((e.get('phase')for e in reversed(events)if e.get('phase')!='row-input'),None)
  if current_phase and current_phase!=last_phase:
   print('拖出状态：'+current_phase,flush=True);last_phase=current_phase
  time.sleep(.2)
 if end_reason is None:end_reason='window_closed'if child.poll()is not None else'timed_out'
 native_started=any(e.get('phase')=='native-start'for e in events)
 (profile/'stop').touch();closed=time.monotonic()
 try:code=child.wait(timeout=15)
 except subprocess.TimeoutExpired:child.kill();child.wait();raise RuntimeError('Owned drag window failed normal exit')
 with lock:seen=list(requests)
 temporary_exports=list(owned_temp.glob('tomato-drag-*'))
 status='passed'if matched and code==0 and not temporary_exports else('not_exercised'if not native_started and code==0 and not temporary_exports else'failed')
 proof=dict(status=status,endReason=end_reason,nativeDragStarted=native_started,batch=args.batch,initialView=args.view,guiSha256=gui_sha,destination=str(destination),files=[dict(name=n,sha256=hashlib.sha256(b).hexdigest().upper(),matched=matches(n,b))for n,b in objects.items()],signedRequests=seen,normalExit=code==0,closeSeconds=time.monotonic()-closed,sharedConfigurationUnchanged=sha(shared)==before,fixtureConfigurationUnchanged=sha(config)==own_before,osInputInjected=False,savedHostsContacted=False,temporaryExportsRemaining=[str(p)for p in temporary_exports],expectedViews=expected_views,downloadDelaySeconds=args.download_delay,runnerSha256=runner_sha,nativeExportEvents=events,nativeState=last_native_state,streamingProgress=args.progress,firstDestinationDataAt=first_destination_data,nativeCopyDialogVisuallyConfirmed=False)
 write(probe/'validation.json',proof);print(json.dumps(proof,ensure_ascii=False),flush=True)
 if status=='not_exercised':
  print('未记录到系统拖出手势；本轮不计为验收通过。可以重新运行启动脚本。',flush=True)
  raise SystemExit(2)
 assert proof['status']=='passed'and proof['sharedConfigurationUnchanged']and proof['fixtureConfigurationUnchanged']
 if args.batch:assert all(not objects[name]or any(r['name']==name and r['method']=='GET'for r in seen)for name in names),'No real content read for a nonempty batch file'
 else:assert all(any(r['name']==name and r['view']==expected_views[name]for r in seen)for name in names),'No real remote read for dropped file'
finally:
 if child.poll()is None:child.kill();child.wait()
 server.shutdown();stdout.close();stderr.close()
