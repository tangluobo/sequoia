"""Apache-generated Java/Rust HS2 interoperability; all connections are loopback."""
from pathlib import Path
import argparse,hashlib,json,os,re,shutil,subprocess,urllib.request
from contextlib import nullcontext
from hive_kdc import realm

ROOT=Path(__file__).resolve().parents[2]
ARTIFACTS=[
    ('org/apache/hive','hive-service-rpc','4.1.0','541b426e10bf35d0d48bbc06c1ebeea97de2411aebd3ab31cab850053aa9dfba'),
    ('org/apache/thrift','libthrift','0.16.0','b311ad08f7dc5ba90139910ec8619460914a4a2952f738603f1d7803ffdbe0a8'),
    ('org/slf4j','slf4j-api','1.7.36','d3ef575e3e4979678dc01bf1dcce51021493b4d11fb7f1be8ad982877c16a1c0'),
]
TEST='database::hive::tests::java::apache_generated_java_thrift_peer_verifies_plain_and_sasl_native_clients'
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--label',default='ci');parser.add_argument('--offline',action='store_true')
    parser.add_argument('--package',choices=['tomato-services'])
    parser.add_argument('--kerberos',action='store_true',help='Own a Unix loopback MIT realm and verify all three GSSAPI security layers')
    args=parser.parse_args()
    if not re.fullmatch(r'[A-Za-z0-9_-]{1,80}',args.label): raise ValueError('Invalid probe label')
    output=ROOT/'target'/'hive-interop'/args.label;output.mkdir(parents=True,exist_ok=True)
    jars=[];manifest=[]
    for group,name,version,digest in ARTIFACTS:
        url=f'https://repo.maven.apache.org/maven2/{group}/{name}/{version}/{name}-{version}.jar'
        path=output/f'{name}-{version}.jar'
        if path.exists(): content=path.read_bytes()
        elif args.offline: raise RuntimeError(f'Missing verified cached fixture: {path.name}')
        else: content=urllib.request.urlopen(url,timeout=60).read(16*1024*1024+1)
        if len(content)>16*1024*1024 or hashlib.sha256(content).hexdigest()!=digest: raise RuntimeError('Fixture checksum mismatch: '+path.name)
        if not path.exists(): path.write_bytes(content)
        jars.append(str(path));manifest.append({'url':url,'sha256':digest,'bytes':len(content)})
    source=ROOT/'crates/services/tests/java/HiveThriftPeer.java'
    classes=output/'classes';classes.mkdir(exist_ok=True)
    javac=Path(os.environ['JAVA_HOME'])/'bin'/('javac.exe' if os.name=='nt' else 'javac') if os.environ.get('JAVA_HOME') else shutil.which('javac')
    if not javac: raise RuntimeError('A JDK is required only for this independent QA peer')
    subprocess.run([str(javac),'-encoding','UTF-8','-cp',os.pathsep.join(jars),'-d',str(classes),str(source)],cwd=ROOT,check=True,creationflags=0x08000000 if os.name=='nt' else 0)
    env=os.environ.copy();env['TOMATO_HIVE_JAVA_CLASSPATH']=os.pathsep.join([str(classes),*jars])
    command=['cargo','test',*(['-p',args.package] if args.package else ['--workspace']),'--lib','--locked',*(['--offline'] if args.offline else []),TEST,'--','--ignored','--exact','--nocapture']
    log=output/'test.log'
    with (realm(output) if args.kerberos else nullcontext(env)) as runtime_env:
        runtime_env['TOMATO_HIVE_JAVA_CLASSPATH']=env['TOMATO_HIVE_JAVA_CLASSPATH']
        with log.open('w',encoding='utf-8') as stream: result=subprocess.run(command,cwd=ROOT,env=runtime_env,stdout=stream,stderr=subprocess.STDOUT)
    text=log.read_text(encoding='utf-8')
    if result.returncode or f'test {TEST} ... ok' not in text: raise RuntimeError('Hive interoperability did not pass: '+str(log))
    proof={'passed':True,'modes':['NOSASL','SASL PLAIN','HTTP Basic session cookie','HTTP Basic expired cookie','HTTP Basic without cookie'],'host':'127.0.0.1','savedConnectionsUsed':False,'savedConfigurationModified':False,'fixtureSourceSha256':hashlib.sha256(source.read_bytes()).hexdigest(),'artifacts':manifest,'log':str(log)}
    if args.kerberos:proof['modes']+=['GSSAPI auth','GSSAPI auth-int','GSSAPI auth-conf','HTTP Kerberos session cookie','HTTP Kerberos expired cookie','HTTP Kerberos fresh per-request tickets']
    report=output/'validation.json';report.write_text(json.dumps(proof,indent=2),encoding='utf-8')
    print(report)
if __name__=='__main__': main()
