"""Collect the pinned Go module and toolchain licenses for cloud binary packages."""
import argparse,json,pathlib,subprocess
p=argparse.ArgumentParser();p.add_argument("--go",default="go");p.add_argument("--output",required=True);a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[1]/"server"
def run(*args):return subprocess.check_output([a.go,*args],cwd=root,text=True,encoding="utf-8")
raw=run("list","-m","-json","all");decoder=json.JSONDecoder();modules=[]
while raw.strip():
    raw=raw.lstrip();value,end=decoder.raw_decode(raw);modules.append(value);raw=raw[end:]
text=["Tomato Cloud: Go runtime and dependency license notices.\n"]
runtime=set(run("list","-deps","-f","{{if .Module}}{{.Module.Path}}{{end}}","./cmd/tomato-cloud").splitlines());count=0
goroot=pathlib.Path(run("env","GOROOT").strip());text += ["\nGo toolchain / standard library\n",(goroot/"LICENSE").read_text(encoding="utf-8")]
for module in modules:
    if module.get("Main") or module["Path"] not in runtime:continue
    count+=1
    directory=pathlib.Path(module["Dir"])
    files=sorted(f for f in directory.iterdir() if f.is_file() and (f.name.upper().startswith(("LICENSE","COPYING","NOTICE")) or f.name.upper()=="COPYRIGHT"))
    if not files:raise RuntimeError("Missing module license: "+module["Path"])
    text.append("\n\n"+module["Path"]+" "+module["Version"]+"\n")
    for f in files:text += ["\n"+f.name+"\n",f.read_text(encoding="utf-8",errors="replace")]
out=pathlib.Path(a.output);out.parent.mkdir(parents=True,exist_ok=True);out.write_text("".join(text),encoding="utf-8")
print(json.dumps({"output":str(out),"modules":count,"bytes":out.stat().st_size}))
