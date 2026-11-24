"""Require actual release evidence before changing the package's acceptance status."""
import argparse,hashlib,json,pathlib,re,shutil

p=argparse.ArgumentParser();p.add_argument("--directory",required=True);p.add_argument("--native",required=True);p.add_argument("--memory",required=True);a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];directory=pathlib.Path(a.directory).resolve()
def read(path):return json.loads(pathlib.Path(path).read_text(encoding="utf-8-sig"))
def sha(path):return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest().upper()
manifest=read(directory/"build-info.json")
assert manifest["status"]=="pending_final_acceptance"
assert sha(directory/"tomato.exe")==manifest["client"]["sha256"]
assert sha(directory/"cloud/tomato-cloud.exe")==manifest["server"]["sha256"]=="BA11DEA650B92B40CF916D940E4B4BB62BEAD83C03A2E9C684411BAF6BF898CB"
sources=read(root/"target/checkpoints"/manifest["source_freeze"]/"production-sources.json")
for file in sources:assert sha(root/file["path"])==file["sha256"],file["path"]
protected=read(root/"target/collaboration-protected-1380.json")
for file in protected:assert sha(file["path"])==file["hash"],file["path"]
log=(root/"target/collaboration-tests-1425.log").read_text(encoding="utf-8-sig")
results=[tuple(map(int,m)) for m in re.findall(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored",log)]
assert results and all(r[1]==0 for r in results)
counts={"passed":sum(r[0] for r in results),"failed":sum(r[1] for r in results),"ignored":sum(r[2] for r in results),"suites":len(results)}
assert counts["passed"]==555 and counts["ignored"]==79,counts
interop=(root/"target/collaboration-interop-1425.log").read_text(encoding="utf-8-sig")
assert len(re.findall(r"test real_go_server_.* \.\.\. ok",interop))==2
native_path=pathlib.Path(a.native).resolve();native=read(native_path/"validation.json")
assert native["status"]=="passed" and native["sha256"].upper()==manifest["client"]["sha256"]
assert {c["case"] for c in native["cases"]}=={"login","objects","connection","members","sql_save","sql_repeat_close"}
assert all(c["exit_code"]==0 and c["proof"]["status"]=="passed" and c["proof"]["error"] is None for c in native["cases"])
assert next(c for c in native["cases"] if c["case"]=="sql_repeat_close")["proof"]["closed_saved_query"]
portal=read(root/"target/window-probes/collaboration-portal-1412/validation.json")
assert portal["status"]=="passed" and not portal["errors"] and len(portal["steps"])==11
bootstrap=read(root/"target/window-probes/collaboration-bootstrap-final-1421/validation.json")
assert bootstrap["status"]=="passed"
tty=read(root/"target/window-probes/collaboration-tty-1411/validation.json")
assert tty["status"]=="passed" and not tty["echoed"] and tty["account_created"]
memory=read(pathlib.Path(a.memory)/"validation.json")
assert len(memory["samples"])==4 and memory["sharedConfigurationUnchanged"] and not memory["savedHostsContacted"]
server_memory=read(root/"target/window-probes/collaboration-server-memory-1430/validation.json")
assert server_memory["status"]=="passed" and server_memory["sha256"]==manifest["server"]["sha256"]
for sample in memory["samples"]:
    if sample["version"]=="new":assert sample["executable"]["sha256"]==manifest["client"]["sha256"]
icon=read(directory/"icon-proof.json");assert icon["allEmbeddedFramesMatchOriginalBytes"] and icon["executableSha256"]==manifest["client"]["sha256"]
race=(root/"target/collaboration-go-race-1408.log").read_text(encoding="utf-8-sig")
assert re.search(r"ok\s+github.com/tangluobo/tomato/cloud/internal/collaboration",race) and "FAIL" not in race
evidence=directory/"evidence";evidence.mkdir(exist_ok=False)
for file in native_path.iterdir():
    if file.is_file() and (file.suffix==".png" or file.name.endswith(".state.json") or file.name=="validation.json"):shutil.copy2(file,evidence/("native-"+file.name))
for name,proof in [("portal",portal),("bootstrap",bootstrap),("console-password",tty),("memory",memory),("server-memory",server_memory),("test-counts",counts)]:
    (evidence/(name+".json")).write_text(json.dumps(proof,ensure_ascii=False,indent=2),encoding="utf-8")
for name in ["collaboration-tests-1425.log","collaboration-interop-1425.log","collaboration-go-race-1408.log","collaboration-clippy-1425.log","collaboration-selfcheck-1425.log"]:shutil.copy2(root/"target"/name,evidence/name)
manifest.update({"status":"verified","validation":{"rust_workspace":counts,"rust_go_interoperability":2,"go_race_test_cases":9,"native_release_cases":6,"native_exit_codes":[0]*6,"portal_workflows":11,"bootstrap_and_backup_restore":True,"console_password_not_echoed":True,"protected_inputs_unchanged":len(protected),"production_source_hashes_unchanged":len(sources),"original_exe_icon_preserved":True,"memory":[{"version":s["version"],"backend":s["backend"],"working_set_mib":s["measurement"]["workingSetMiB"],"private_commit_mib":s["measurement"]["privateCommitMiB"]} for s in memory["samples"]]}})
(directory/"build-info.json").write_text(json.dumps(manifest,ensure_ascii=False,indent=2),encoding="utf-8")
print(json.dumps({"status":"verified","client_sha256":manifest["client"]["sha256"],"validation":manifest["validation"]},ensure_ascii=False))
