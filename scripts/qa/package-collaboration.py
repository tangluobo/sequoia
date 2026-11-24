"""Create a reviewable portable package, retaining the prior release's license payload."""
import argparse,hashlib,json,pathlib,shutil,zipfile
p=argparse.ArgumentParser();p.add_argument("--client",required=True);p.add_argument("--server",required=True);p.add_argument("--base",required=True);p.add_argument("--checkpoint",required=True);p.add_argument("--output",required=True);a=p.parse_args();root=pathlib.Path(__file__).resolve().parents[2];out=pathlib.Path(a.output).resolve();out.mkdir(parents=True,exist_ok=False)
sources=json.loads((root/"target/checkpoints"/a.checkpoint/"production-sources.json").read_text(encoding="utf-8-sig"))
shutil.copy2(a.client,out/"tomato.exe");(out/"cloud").mkdir();shutil.copy2(a.server,out/"cloud/tomato-cloud.exe");shutil.copy2(root/"server/README.md",out/"cloud/README.md");shutil.copy2(root/"docs/collaboration-research.md",out/"collaboration-research.md");shutil.copy2(root/"docs/third-party/cloud-server-NOTICES.txt",out/"cloud/NOTICES.txt")
def sha(path):
    h=hashlib.sha256()
    with open(path,"rb")as f:
        for data in iter(lambda:f.read(65536),b""):h.update(data)
    return h.hexdigest().upper()
manifest={"status":"pending_final_acceptance","scope":"self-hosted Tomato Cloud; independent accounts","source_freeze":a.checkpoint,"production_source_files":len(sources),"client":{"path":"tomato.exe","sha256":sha(out/"tomato.exe"),"bytes":(out/"tomato.exe").stat().st_size},"server":{"path":"cloud/tomato-cloud.exe","sha256":sha(out/"cloud/tomato-cloud.exe"),"bytes":(out/"cloud/tomato-cloud.exe").stat().st_size,"toolchain":"go1.26.9"},"legacy_configuration":"not packaged; original encrypted files remain in place","limitations":["Single-server SQLite deployment","No Navicat official-account or proprietary file editor integration","Live SMTP, public DNS/TLS, Docker and Linux runtime require deployment-environment verification"]}
(out/"build-info.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2),encoding="utf-8")
(out/"SHA256SUMS.txt").write_text(manifest["client"]["sha256"]+"  tomato.exe\n"+manifest["server"]["sha256"]+"  cloud/tomato-cloud.exe\n",encoding="utf-8")
print(json.dumps({"directory":str(out),"client_sha256":manifest["client"]["sha256"],"server_sha256":manifest["server"]["sha256"]}))
# Final archive is deliberately created only after final acceptance updates build-info.
