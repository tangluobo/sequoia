"""Run the independent synthetic FreeRDP Standard Security peer, never a user desktop."""
from pathlib import Path
import argparse, ctypes, hashlib, json, os, re, socket, subprocess, time
from rdp_proxy import negotiation

ROOT=Path(__file__).resolve().parents[2]
TEST='independent_freerdp_standard_security_draws_accepts_input_and_closes'
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-exe',required=True,type=Path)
    parser.add_argument('--toolchain',required=True,type=Path)
    parser.add_argument('--label',required=True)
    parser.add_argument('--strength',choices=['128','56','40','low','high','none'],default='128')
    args=parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9_-]{1,80}',args.label):raise ValueError('Invalid fixture label')
    test=args.test_exe.resolve()
    if not test.is_relative_to(ROOT/'target') or not re.fullmatch(r'remote_desktop-[0-9a-f]+\.exe',test.name):raise ValueError('Use a built workspace integration test')
    fixture=ROOT/'target/freerdp-fixture-1672';build=fixture/'build'
    server=build/'server/Sample/sfreerdp-server.exe'
    if not server.is_file():raise ValueError('Build the pinned loopback-only synthetic sample first')
    sample=(fixture/'source/freerdp-3.32.2/server/Sample/sfreerdp.c').read_text(encoding='utf-8')
    if 'instance->Open(instance, "127.0.0.1", (UINT16)port)' not in sample or 'freerdp_settings_set_bool(settings, FreeRDP_TlsSecurity, FALSE)' not in sample:raise ValueError('Synthetic sample must require Standard RDP on loopback')
    output=ROOT/'target/rdp-standard-interop'/args.label;output.mkdir(parents=True,exist_ok=False)
    profile=output/'profile';profile.mkdir()
    env=os.environ.copy();env['PATH']=os.pathsep.join([str(fixture/'sdk/ucrt64/bin'),*[str(p)for p in sorted({p.parent for p in build.rglob('*.dll')})],str(args.toolchain),env['PATH']])
    env.update(APPDATA=str(profile),LOCALAPPDATA=str(profile),HOME=str(profile),TOMATO_DATA_DIR=str(profile),TMPDIR=str(output),WLOG_LEVEL='INFO',TOMATO_RDP_FIXTURE_STRENGTH=args.strength)
    if os.name=='nt':ctypes.windll.kernel32.SetErrorMode(0x8003)
    openssl=fixture/'sdk/ucrt64/bin/openssl.exe'
    (output/'openssl.cnf').write_text('[req]\ndistinguished_name=dn\n[dn]\nCN=localhost\n',encoding='utf-8')
    subprocess.run([str(openssl),'req','-config',str(output/'openssl.cnf'),'-x509','-newkey','rsa:2048','-nodes','-keyout',str(output/'server.key'),'-out',str(output/'server.crt'),'-subj','/CN=localhost','-days','1'],env=env,cwd=output,check=True,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,creationflags=0x08000000)
    with socket.socket() as reserve:reserve.bind(('127.0.0.1',0));port=reserve.getsockname()[1]
    command=[str(server),'--port='+str(port),'--cert='+str(output/'server.crt'),'--key='+str(output/'server.key')]
    with (output/'server.log').open('wb') as serverlog:
        owned=subprocess.Popen(command,env=env,cwd=output,stdout=serverlog,stderr=subprocess.STDOUT,creationflags=0x08000000)
        try:
            deadline=time.monotonic()+15
            while True:
                if owned.poll() is not None:raise RuntimeError('Synthetic sample exited: '+str(output/'server.log'))
                try:
                    with socket.create_connection(('127.0.0.1',port),timeout=.2):break
                except OSError:
                    if time.monotonic()>deadline:raise RuntimeError('Synthetic sample did not listen')
                    time.sleep(.05)
            expected={'128':(2,2),'56':(8,2),'40':(1,2),'low':(2,1),'high':(2,3),'none':(0,0)}[args.strength]
            mask={'56':8,'40':1}.get(args.strength)
            with negotiation(port,mask,expected) as (route,selected,errors):
                env.update(TOMATO_TEST_FREERDP_SAMPLE='owned-loopback-synthetic',TOMATO_TEST_FREERDP_SAMPLE_PORT=str(route))
                with (output/'test.log').open('w',encoding='utf-8') as log:
                    result=subprocess.run([str(test),'--exact',TEST,'--ignored','--nocapture'],env=env,cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,timeout=180,creationflags=0x08000000)
            (output/'negotiation.json').write_text(json.dumps({'selected':selected,'errors':errors},indent=2),encoding='utf-8')
            text=(output/'test.log').read_text(encoding='utf-8')
            if result.returncode or 'test result: ok. 1 passed; 0 failed' not in text or TEST not in text:raise RuntimeError('Independent RDP interoperability failed: '+str(output/'test.log'))
            if errors or len(selected)!=2 or any((p['method'],p['level'])!=expected for p in selected):raise RuntimeError('Actual negotiated security did not match requested QA mode')
            accepted=re.findall(r'standard-security-acceptance initial_tls=(true|false) sent=(\d+) cleanup_ms=([0-9.]+)',text)
            if {a for a,_,_ in accepted}!={'true','false'} or len(accepted)!=2 or any(int(n)<4105 or float(ms)>=2000 for _,n,ms in accepted):raise RuntimeError('Native input or cleanup acceptance evidence missing')
        finally:
            if owned.poll() is None:owned.terminate()
            try:owned.wait(timeout=5)
            except subprocess.TimeoutExpired:owned.kill();owned.wait(timeout=5)
    proof={'passed':True,'peer':'FreeRDP 3.32.2 synthetic sample','host':'127.0.0.1','nativeClient':True,'strength':args.strength,'negotiatedSecurity':selected,'weakClientAdvertisementMask':mask,'modes':[{'initialUseSsl':a=='true','sentPackets':int(n),'cleanupMs':float(ms)}for a,n,ms in accepted],'savedConnectionsUsed':False,'systemClipboardWritten':False,'userDesktopCaptured':False,'serverSha256':hashlib.sha256(server.read_bytes()).hexdigest(),'testExeSha256':hashlib.sha256(test.read_bytes()).hexdigest(),'sources':json.loads((fixture/'source.json').read_text()),'log':str(output/'test.log')}
    path=output/'validation.json';path.write_text(json.dumps(proof,indent=2),encoding='utf-8');print(path)
if __name__=='__main__':main()
