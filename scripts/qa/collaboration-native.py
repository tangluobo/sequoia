"""Native captures and real local Go/SQLite collaboration workflows, isolated per case."""
import argparse, base64, hashlib, json, os, pathlib, shutil, socket, sqlite3, subprocess, time, urllib.request

p=argparse.ArgumentParser();p.add_argument("--exe",required=True);p.add_argument("--server",required=True);p.add_argument("--output",required=True);p.add_argument("--cases",default="login,objects,connection,members,sql_save,sql_repeat_close");a=p.parse_args()
out=pathlib.Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False)
source_exe=pathlib.Path(a.exe).resolve();exe=out/"tomato.exe";shutil.copy2(source_exe,exe);server_exe=pathlib.Path(a.server).resolve();fingerprint=hashlib.sha256(exe.read_bytes()).hexdigest();results=[]
for case in a.cases.split(","):
    root=out/("tomato-cloud-fixture-"+case);root.mkdir();screen=out/(case+".png")
    with sqlite3.connect(root/"owned.sqlite") as db:db.execute("CREATE TABLE owned(id INTEGER)");db.execute("INSERT INTO owned VALUES(1)")
    with socket.socket() as sock:sock.bind(("127.0.0.1",0));port=sock.getsockname()[1]
    address="http://127.0.0.1:"+str(port);env=dict(os.environ);env["TOMATO_CLOUD_SMTP_ADDRESS"]=""
    log=open(root/"server.log","wb");server=subprocess.Popen([str(server_exe),"--data",str(root/"server"),"--listen","127.0.0.1:"+str(port),"--local-fixture-registration"],stdout=log,stderr=log,env=env,creationflags=getattr(subprocess,"CREATE_NO_WINDOW",0))
    app=None
    def call(path,method="GET",body=None,token=""):
        data=None if body is None else json.dumps(body,ensure_ascii=False).encode()
        request=urllib.request.Request(address+path,data=data,method=method,headers={"Content-Type":"application/json","Authorization":"Bearer "+token})
        with urllib.request.urlopen(request,timeout=10) as r:return json.load(r)
    try:
        for _ in range(100):
            try:call("/healthz");break
            except OSError:time.sleep(.05)
        for email in ("owner@example.test","editor@example.test"):call("/v1/register","POST",{"email":email,"name":"验收账号","password":"fixture-password-123"})
        token=call("/v1/login","POST",{"email":"owner@example.test","password":"fixture-password-123","code":"","device":"owned native QA"})["token"]
        project=call("/v1/projects","POST",{"name":"协作验收"},token)
        def obj(id,kind,name,body,parent=""):
            return call("/v1/projects/"+project["id"]+"/objects/"+id,"PUT",{"parent_id":parent,"kind":kind,"name":name,"body":body,"base_revision":0,"mutation_id":"seed-"+id,"deleted":False},token)
        obj("group","group","项目连接",{})
        obj("connection","connection","SQLite 验收",{"type":"SQLITE","database":str(root/"owned.sqlite")},"group")
        obj("query","query","共享查询",{"text":"SELECT 1 AS value","database":str(root/"owned.sqlite")},"connection")
        obj("snippet","snippet","常用 SQL",{"text":"SELECT CURRENT_TIMESTAMP"},"connection")
        obj("pipeline","pipeline","聚合管道",{"text":"[{\"$limit\":10}]"})
        obj("model","model","模型工作区",{"filename":"owned.model","base64":base64.b64encode(b"owned-workspace").decode()})
        obj("bi","bi","BI 工作区",{"filename":"owned.bi","base64":base64.b64encode(b"owned-dashboard").decode()})
        env.update({"TOMATO_DATA_DIR":str(root),"TOMATO_UI_PREVIEW":"collaboration","TOMATO_CLOUD_TEST_URL":address,"TOMATO_CLOUD_TEST_CASE":case,"TOMATO_UI_CAPTURE":str(screen),"TOMATO_UI_CAPTURE_DELAY_MS":"7000","TOMATO_UI_SCALE":"1.0"})
        with open(root/"application.log","wb") as app_log:
            started=time.monotonic();app=subprocess.Popen([str(exe)],env=env,stdout=app_log,stderr=app_log)
            app.wait(timeout=40)
        assert app.returncode==0,(case,app.returncode,(root/"application.log").read_text(errors="replace")[-3000:])
        assert screen.exists() and screen.stat().st_size>10000,(case,"missing native framebuffer")
        proof=json.loads(screen.with_suffix(".state.json").read_text(encoding="utf-8"));assert proof["status"]=="passed" and proof["error"] is None,proof
        if case.startswith("sql_"):assert call("/v1/projects/"+project["id"]+"/objects/query",token=token)["body"]["text"]=="SELECT 'cloud-native-saved🙂' AS value"
        if case=="sql_repeat_close":assert proof["closed_saved_query"]
        assert hashlib.sha256(exe.read_bytes()).hexdigest()==fingerprint,"executable changed"
        results.append({"case":case,"status":"passed","exit_code":app.returncode,"duration_seconds":round(time.monotonic()-started,3),"proof":proof,"screenshot":str(screen)})
        print(case+": passed",flush=True)
    finally:
        if app is not None and app.poll() is None:app.terminate();app.wait()
        if server.poll() is None:server.terminate();server.wait()
        log.close()
(out/"validation.json").write_text(json.dumps({"status":"passed","executable":str(exe),"sha256":fingerprint,"cases":results},ensure_ascii=False,indent=2),encoding="utf-8")
