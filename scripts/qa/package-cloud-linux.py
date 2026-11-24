"""Package the cross-compiled server, deployment source and notices; no runtime claim."""
import argparse, hashlib, json, pathlib, tarfile

p=argparse.ArgumentParser()
p.add_argument("--server",required=True)
p.add_argument("--checkpoint",required=True)
p.add_argument("--output",required=True)
a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2]
out=pathlib.Path(a.output).resolve()
out.mkdir(parents=True,exist_ok=False)
binary=pathlib.Path(a.server).resolve()
digest=hashlib.sha256(binary.read_bytes()).hexdigest().upper()
manifest={"status":"cross_compiled","platform":"linux/amd64","toolchain":"go1.26.9","static":True,"runtime_executed":False,"sha256":digest,"bytes":binary.stat().st_size,"source_freeze":a.checkpoint,"deployment":"single backend instance; configure TLS or an isolated HTTPS reverse proxy; see server/README.md"}
(out/"build-info.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2),encoding="utf-8")
(out/"SHA256SUMS.txt").write_text(digest+"  tomato-cloud\n",encoding="utf-8")
archive=out/"tomato-cloud-linux-amd64.tar.gz"
with tarfile.open(archive,"x:gz",compresslevel=6) as tar:
    def add(source,name,executable=False):
        info=tar.gettarinfo(str(source),arcname="tomato-cloud-linux-amd64/"+name)
        info.uid=info.gid=0;info.uname=info.gname="";info.mode=0o755 if executable else 0o644
        with open(source,"rb") as stream:tar.addfile(info,stream)
    add(binary,"tomato-cloud",True)
    add(out/"build-info.json","build-info.json")
    add(out/"SHA256SUMS.txt","SHA256SUMS.txt")
    for file in sorted((root/"server").rglob("*")):
        if not file.is_file() or any(x in file.parts for x in ["cloud-data","secrets",".git"]) or file.suffix in [".exe",".db",".key"]:continue
        add(file,"server/"+file.relative_to(root/"server").as_posix())
    add(root/"LICENSE","LICENSE")
    add(root/"docs/third-party/cloud-server-NOTICES.txt","NOTICES.txt")
    add(root/"docs/collaboration-research.md","docs/collaboration-research.md")
with tarfile.open(archive) as tar:
    prefix="tomato-cloud-linux-amd64/"
    entry=tar.getmember(prefix+"tomato-cloud")
    assert entry.mode==0o755
    assert hashlib.sha256(tar.extractfile(entry).read()).hexdigest().upper()==digest
    names=tar.getnames();assert len(names)==len(set(names))
print(json.dumps({"archive":str(archive),"bytes":archive.stat().st_size,"entries":len(names),"runtime_executed":False}))
