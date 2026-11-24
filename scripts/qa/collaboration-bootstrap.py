"""Exercise packaged server bootstrap, admin provisioning and an online backup restore."""
import argparse,json,os,pathlib,socket,subprocess,time,urllib.request
p=argparse.ArgumentParser();p.add_argument("--server",required=True);p.add_argument("--output",required=True);a=p.parse_args();exe=str(pathlib.Path(a.server).resolve());out=pathlib.Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False);data=out/"data";backup=out/"backup";env=dict(os.environ);env["TOMATO_CLOUD_SMTP_ADDRESS"]="";password="fixture-bootstrap-password-123"
def cli(*args,secret=None):
    r=subprocess.run([exe,"--data",str(data),*args],input=None if secret is None else (secret+"\n").encode(),stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0),timeout=25)
    assert r.returncode==0,r.stderr.decode(errors="replace");return r
cli("--command","create-admin","--email","admin@example.test",secret=password)
def start(directory):
    with socket.socket()as sock:sock.bind(("127.0.0.1",0));port=sock.getsockname()[1]
    base="http://127.0.0.1:"+str(port);log=open(out/(directory.name+".log"),"wb");child=subprocess.Popen([exe,"--data",str(directory),"--listen","127.0.0.1:"+str(port)],env=env,stdout=log,stderr=log,creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
    for _ in range(100):
        try:
            with urllib.request.urlopen(base+"/healthz",timeout=1)as r:
                if r.status==200:return child,base,log
        except OSError:time.sleep(.05)
    child.terminate();child.wait();log.close();raise RuntimeError("server startup failed")
def call(base,path,body=None,token="",method=None):
    raw=None if body is None else json.dumps(body).encode();request=urllib.request.Request(base+"/v1"+path,data=raw,method=method or("GET"if body is None else"POST"),headers={"Content-Type":"application/json","Authorization":"Bearer "+token})
    with urllib.request.urlopen(request,timeout=10)as r:return json.load(r)
server,base,log=start(data)
try:
    login=call(base,"/login",{"email":"admin@example.test","password":password,"code":"","device":"owned bootstrap QA"});assert login["user"]["admin"]
    token=login["token"];call(base,"/admin/users",{"email":"member@example.test","name":"member","password":password},token)
    member=call(base,"/login",{"email":"member@example.test","password":password,"code":"","device":"member"});assert not member["user"]["admin"]
    project=call(base,"/projects",{"name":"backup proof"},token);cli("--command","backup","--output",str(backup))
finally:server.terminate();server.wait();log.close()
restored,base,log=start(backup)
try:
    login=call(base,"/login",{"email":"admin@example.test","password":password,"code":"","device":"restore QA"});projects=call(base,"/projects",token=login["token"]);assert any(p["id"]==project["id"]for p in projects)
finally:restored.terminate();restored.wait();log.close()
(out/"validation.json").write_text(json.dumps({"status":"passed","server":exe,"checks":["hidden/stdin bootstrap password","verified administrator login","admin creates non-admin account","registration-disabled user login","online SQLite backup","database and key restore","restored project identity"]},indent=2),encoding="utf-8")
print("bootstrap and online backup restore: passed")
