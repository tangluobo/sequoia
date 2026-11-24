"""Finalize the already verified client/server bundle without rebuilding source."""
import argparse,json,pathlib,shutil,zipfile
p=argparse.ArgumentParser();p.add_argument("--directory",required=True);p.add_argument("--base",required=True);a=p.parse_args();root=pathlib.Path(__file__).resolve().parents[2];directory=pathlib.Path(a.directory).resolve();manifest=json.loads((directory/"build-info.json").read_text(encoding="utf-8"));assert manifest["status"]=="verified","final acceptance is required before archiving"
archive=directory/"tomato-cloud-portable.zip"
if archive.exists():raise RuntimeError("Archive already exists")
with zipfile.ZipFile(a.base)as previous,zipfile.ZipFile(archive,"x",compression=zipfile.ZIP_DEFLATED,compresslevel=6)as target:
    replaced={"tomato.exe","build-info.json","SHA256SUMS.txt","icon-proof.json"}
    for entry in previous.infolist():
        if entry.filename in replaced:continue
        current=root/entry.filename.removeprefix("notices/")
        if entry.filename.startswith("notices/")and current.is_file():target.write(current,entry.filename)
        else:
            with previous.open(entry)as source,target.open(entry.filename,"w")as dest:shutil.copyfileobj(source,dest,65536)
    for file in sorted(directory.rglob("*")):
        if not file.is_file()or file==archive:continue
        target.write(file,file.relative_to(directory).as_posix())
    for file in sorted((root/"server").rglob("*")):
        if not file.is_file()or any(x in file.parts for x in["cloud-data","secrets",".git"])or file.suffix in[".exe",".db",".key"]:continue
        target.write(file,"source/server/"+file.relative_to(root/"server").as_posix())
    target.write(root/"LICENSE","source/LICENSE")
    for name in["collaboration-research.md","migration.md","performance.md"]:
        target.write(root/"docs"/name,"docs/"+name);target.write(root/"docs"/name,"source/docs/"+name)
    target.write(root/"scripts/build-cloud-notices.py","source/scripts/build-cloud-notices.py")
with zipfile.ZipFile(archive)as verify:
    assert verify.testzip()is None
    names=verify.namelist();assert len(names)==len(set(names)),"duplicate archive entries"
    assert "cloud/tomato-cloud.exe"in names and"source/server/go.mod"in names
print(json.dumps({"archive":str(archive),"bytes":archive.stat().st_size,"entries":len(names)}))
