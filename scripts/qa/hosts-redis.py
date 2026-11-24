"""Current original Hosts/Redis components, actual CLI backend and owned GUI frames."""
from pathlib import Path
import argparse,hashlib,json,os,re,shutil,socket,subprocess,time
ROOT=Path(__file__).resolve().parents[2]

def sha(p):
    with p.open('rb') as f:return hashlib.file_digest(f,'sha256').hexdigest().upper()

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe',required=True);parser.add_argument('--cli',required=True);parser.add_argument('--label',required=True)
    parser.add_argument('--hosts-sudo',action='store_true',help='Capture the password prompt with an owned Hosts fixture')
    args=parser.parse_args();assert re.fullmatch('[a-z0-9-]{1,80}',args.label)
    gui=Path(args.exe).resolve(strict=True);cli=Path(args.cli).resolve(strict=True)
    assert gui.name=='tomato.exe'and cli.name=='tomato-cli.exe'
    assert all(p.is_relative_to(ROOT/'target')or p.is_relative_to(ROOT/'dist')for p in [gui,cli])
    out=ROOT/'target/window-probes'/args.label;out.mkdir(parents=True,exist_ok=False)
    shared={p:sha(p)for p in (Path.home()/'.tomato').glob('*.json')if p.name!='downloads.json'}
    env={k:v for k,v in os.environ.items()if not k.startswith('TOMATO_')}
    profile=out/'tomato-hosts-redis-fixture-cli';profile.mkdir();env['TOMATO_DATA_DIR']=str(profile)
    def envelope(command):
        name,params=next(iter(command.items()));return dict(command={'Hosts':'hosts','RedisData':'redis_data'}[name],params=params)
    def run(command):
        result=subprocess.run([str(cli),'run'],input=json.dumps(envelope(command),ensure_ascii=False).encode('utf-8'),capture_output=True,cwd=ROOT,env=env,timeout=45,creationflags=0x08000000)
        if result.returncode:raise RuntimeError(result.stderr.decode('utf-8',errors='replace'))
        return {next(iter(command)):json.loads(result.stdout)['data']}
    hosts=out/'hosts';hosts.mkdir();system=hosts/'hosts';system.write_text('# owned user line\n127.0.0.1 localhost\n',encoding='utf-8');config=hosts/'hosts_groups.json'
    config.write_text(json.dumps(dict(groups=[dict(name='开发验收',enabled=False,entries=[dict(ip='10.0.0.2',domain='dev.test alias # 中文',futureEntry=7),dict(ip='::1',domain='ipv6.test')],futureGroup=True)],future={'preserve':True}),ensure_ascii=False),encoding='utf-8')
    paths=dict(config_path=str(config),system_path=str(system))
    previous=run({'Hosts':dict(**paths,action='Load')})['Hosts'];candidate=previous['config'];candidate['groups'][0]['enabled']=True
    saved=run({'Hosts':dict(**paths,action={'Save':dict(previous=previous,config=candidate,system=None,apply=True)})})['Hosts']
    saved_bytes=config.read_bytes();assert b'"groups":['in saved_bytes and b'"entries":['in saved_bytes
    assert saved['config']['groups'][0]['entries'][0]['futureEntry']==7 and saved['config']['future']['preserve']
    assert '# 开发验收\n10.0.0.2 dev.test alias # 中文\n::1 ipv6.test\n# End 开发验收'in system.read_text(encoding='utf-8')
    assert config.with_suffix('.json.bak').is_file()
    # An external edit must win; an unsuccessful save must not publish a new config.
    system.write_text('external owned edit',encoding='utf-8');stamp=sha(config)
    conflict={'Hosts':dict(**paths,action={'Save':dict(previous=saved,config=saved['config'],system=None,apply=True)})}
    failed=subprocess.run([str(cli),'run'],input=json.dumps(envelope(conflict),ensure_ascii=False).encode('utf-8'),capture_output=True,cwd=ROOT,env=env,timeout=45,creationflags=0x08000000)
    assert failed.returncode!=0 and '其他程序修改'in failed.stderr.decode('utf-8')and sha(config)==stamp and system.read_text()=='external owned edit'
    system.write_text(saved['system'],encoding='utf-8')
    with socket.socket()as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
    server=ROOT/'target/redis-fixture/bin/redis-server.exe';assert server.is_file()
    process=subprocess.Popen([str(server),'--bind','127.0.0.1','--port',str(port),'--save','','--appendonly','no','--maxmemory','32mb'],cwd=profile,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL,creationflags=0x08000000)
    originals=[];captures=[]
    connection=dict(id='owned-redis-1704',name='Redis 验收',type='REDIS',host='127.0.0.1',port=port,redisDatabase=3,password='')
    def redis(action):return run({'RedisData':dict(connection=connection,action=action)})['RedisData']
    try:
        deadline=time.monotonic()+15
        while True:
            try:
                with socket.create_connection(('127.0.0.1',port),timeout=1):break
            except OSError:assert time.monotonic()<deadline;time.sleep(.05)
        for key,kind,text in [('a','string','中文 alpha'),('b','hash','field 中文 value'),('c','list','one\ntwo')]:
            redis({'Write':dict(key=key,kind=kind,text=text,create=True)})
        original=Path('D:/Data/Git/tangluobo/tomata/src/main/java/com/tangluobo/tomato')
        sources=[ROOT/'crates/services/tests/java/HostsRedisOracle.java',ROOT/'crates/services/tests/java/ToolPaneOracle.java',original/'module/tools/HostsFilePane.java',original/'module/connect/view/RedisDataView.java',original/'module/connect/service/RedisService.java']
        identities=[dict(path=str(p),sha256=sha(p))for p in sources]
        classes=out/'classes';classes.mkdir();old=(ROOT/'target/oracle-tools-223/compile.args').read_text(encoding='utf-8-sig').splitlines();cp=str(classes)+';'+old[old.index('-cp')+1].strip('"')
        java=Path('D:/App/Java/graalvm-jdk-25.0.3+9.1/bin')
        def argsfile(path,values):path.write_text('\n'.join('"'+v.replace('\\','/')+'"'for v in values),encoding='utf-8')
        compile_args=out/'compile.args';argsfile(compile_args,['--enable-preview','--release','25','-encoding','UTF-8','-cp',cp,'-d',str(classes),*[str(p)for p in sources]])
        with(out/'javac.log').open('wb')as log:subprocess.run([str(java/'javac.exe'),'@'+str(compile_args)],cwd=ROOT,stdout=log,stderr=subprocess.STDOUT,check=True,timeout=120,creationflags=0x08000000)
        for mode in ['hosts','redis']:
            home=out/('tomato-hosts-redis-oracle-'+mode);(home/'.tomata').mkdir(parents=True)
            (home/'.tomata/hosts_groups.json').write_bytes(saved_bytes)
            destination=out/('java-'+mode);destination.mkdir()
            arguments=out/(mode+'.args');argsfile(arguments,['--enable-preview','-Dprism.order=sw','-Djava.util.prefs.PreferencesFactory=ToolPaneOracle$MemoryFactory','-Duser.home='+str(home),'-cp',cp,'HostsRedisOracle',mode,str(destination),str(port)])
            with(destination/'stdout.log').open('wb')as stdout,(destination/'stderr.log').open('wb')as stderr:subprocess.run([str(java/'java.exe'),'@'+str(arguments)],cwd=ROOT,stdout=stdout,stderr=stderr,check=True,timeout=90,creationflags=0x08000000)
            proof=json.loads((destination/(mode+'.json')).read_text(encoding='utf-8'))
            if mode=='hosts':assert proof['prettyConfigurationRejectedByOriginal']and proof['groups'][0]['name']=='开发验收'
            else:
                assert proof['rowHeight']==30
                menu=proof['multiSelectionMenu'];assert any(m.get('text')=='删除 (2个)'and not m['disabled']for m in menu)
                assert all(m['disabled']for m in menu if m.get('text')in ['重命名','设置TTL'])
            originals.append(dict(case=mode,proof=proof,capture=str(destination/(mode+'.png'))))
        native_modes=['hosts','redis']+(['hosts-sudo'] if args.hosts_sudo else [])
        for mode in native_modes:
            destination=out/('native-'+mode);destination.mkdir();home=destination/('tomato-hosts-redis-fixture-'+mode);home.mkdir()
            cfg=home/'connections.json';cfg.write_text(json.dumps(dict(connections=[connection]if mode=='redis'else[],groups=[],futureOwnedValue={'preserve':True}),ensure_ascii=False),encoding='utf-8');before=sha(cfg)
            childenv=env.copy();childenv.update(TOMATO_DATA_DIR=str(home),TOMATO_UI_CAPTURE=str(destination/'capture.png'),TOMATO_UI_PREVIEW='hosts' if mode=='hosts-sudo' else mode,TOMATO_UI_CAPTURE_FRAMES='2',TOMATO_UI_CAPTURE_DELAY_MS='3500',WGPU_BACKEND='vulkan')
            if mode.startswith('hosts'):childenv['TOMATO_UI_PREVIEW_PATH']=str(hosts)
            if mode=='redis':childenv['TOMATO_UI_PREVIEW_DIALOG']='redis-value'
            if mode=='hosts-sudo':childenv['TOMATO_UI_PREVIEW_DIALOG']='hosts-sudo'
            startup=subprocess.STARTUPINFO();startup.dwFlags=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
            with(destination/'stdout.log').open('wb')as stdout,(destination/'stderr.log').open('wb')as stderr:subprocess.run([str(gui)],cwd=ROOT,env=childenv,stdout=stdout,stderr=stderr,check=True,timeout=90,creationflags=0x08000000,startupinfo=startup)
            capture=destination/'capture-01.png';assert capture.is_file()and sha(cfg)==before
            captures.append(dict(case=mode,capture=str(capture),captureSha256=sha(capture),normalExit=True,configurationUnchanged=True))
        # Exercise the final command engine's five value types, TTL, rename and independent delete.
        for kind,text in [('string','中文 value'),('list','one\ntwo'),('set','one\ntwo'),('hash','field value'),('zset','1 member')]:
            key='roundtrip-'+kind;redis({'Write':dict(key=key,kind=kind,text=text,create=True)})
            value=redis({'Read':dict(key=key)})['Value'];assert value['kind']==kind and (set(value['text'].splitlines())==set(text.splitlines())if kind=='set'else value['text']==text)
            redis({'Ttl':dict(key=key,seconds=600)});assert redis({'Read':dict(key=key)})['Value']['ttl']>590
        redis({'Rename':dict(key='roundtrip-string',name='renamed')})
        redis({'Delete':dict(keys=['renamed','roundtrip-list','roundtrip-set','roundtrip-hash','roundtrip-zset'])})
        assert redis({'Read':dict(key='a')})['Value']['text']=='中文 alpha'
        assert redis({'Read':dict(key='renamed')})['Value']['kind']=='none'
        assert all(sha(Path(s['path']))==s['sha256']for s in identities)
        assert all(sha(p)==h for p,h in shared.items())
        proof=dict(passed=True,guiSha256=sha(gui),cliSha256=sha(cli),sources=identities,originals=originals,nativeWindows=captures,cliActualRedisTypes=5,hostsOriginalParserReadsActualRustSave=True,hostsRevisionConflictPreserved=True,configurationUnchanged=True,ownedLoopbackOnly=True,physicalOsInputUsed=False,systemClipboardWritten=False,fullJavaVisualParity=False)
        (out/'validation.json').write_text(json.dumps(proof,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(dict(passed=True,nativeWindows=len(captures),originalWindows=2,cliActualRedisTypes=5,hostsCompatibility=True)),flush=True)
    finally:
        process.terminate();process.wait(timeout=15)
if __name__=='__main__':main()
