"""Collect the exact table-query release's local QA evidence before packaging."""
import argparse, hashlib, json, pathlib, re, shutil
p=argparse.ArgumentParser();p.add_argument('--candidate',type=pathlib.Path,required=True);p.add_argument('--label',default='1494');p.add_argument('--cloud-label',default='1496');a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];candidate=a.candidate.resolve()
load=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest().upper()
exe=candidate/'tomato.exe';fingerprint=digest(exe)
proofs={
 'native-table':root/f'target/window-probes/table-sql-release-{a.label}/validation.json',
 'table-layout':root/f'target/window-probes/table-sql-release-{a.label}/layout-validation.json',
 'native-cloud':root/f'target/window-probes/table-sql-cloud-{a.cloud_label}/validation.json',
 'memory':root/f'target/window-probes/table-sql-memory-{a.label}/validation.json',
 'database-engines':root/'target/database-fixtures/probes/table-sql-1492/validation.json',
 'embedded-adapters':candidate/'embedded-validation.json','icon':candidate/'icon-validation.json',
}
data={name:load(path) for name,path in proofs.items()}
for name,proof in data.items():
 assert proof.get('status','passed')=='passed',name
 if name in ('native-table','table-layout','native-cloud','embedded-adapters'):
  assert (proof.get('guiSha256') or proof.get('sha256')).upper()==fingerprint,name
assert len(data['native-table']['captures'])==6
assert len(data['table-layout']['checks'])==2 and all(row['toolbarAndGridPixelIdentical'] for row in data['table-layout']['checks'])
assert not data['native-table']['savedHostsContacted'] and data['native-table']['sharedConfigurationUnchanged']
assert len(data['native-cloud']['cases'])==2 and all(row['exit_code']==0 for row in data['native-cloud']['cases'])
assert data['icon']['allEmbeddedFramesMatchOriginalBytes'] and data['icon']['executableSha256']==fingerprint
assert data['memory']['sharedConfigurationUnchanged'] and len(data['memory']['samples'])==4
for sample in data['memory']['samples']:
 if sample['version']=='new':assert sample['executable']['sha256']==fingerprint
workspace=(root/'target/table-sql-workspace-1493.log').read_text(errors='replace')
counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',workspace)
passed,failed,ignored=(sum(int(row[i]) for row in counts) for i in range(3))
assert (passed,failed,ignored)==(574,0,82),(passed,failed,ignored)
for name in ('sql_preview_keyboard_selection_copies_and_cannot_edit_the_statement','sql_preview_context_menu_copies_full_query_even_when_showing_a_status_message'):
 assert name+' ... ok' in workspace,name
assert 'Finished' in (root/'target/table-sql-clippy-1493.log').read_text(errors='replace')
assert (root/'target/table-sql-fmt-1493.log').stat().st_size==0
assert 'Tomato self-check passed' in (root/'target/table-sql-cli-1493.log').read_text()
assert 'test result: ok. 22 passed; 0 failed' in (root/'target/database-fixtures/probes/table-sql-1492/tests.log').read_text(errors='replace')
manifest=load(root/'target/checkpoints/table-sql-1493/production-sources.json')
for entry in manifest:assert digest(root/entry['path'])==entry['sha256'],entry['path']
protected=load(root/'target/table-sql-protected-1492.json')
for entry in protected:assert digest(pathlib.Path(entry['path']))==entry['hash'],entry['path']
server=root/'target/qa-candidate-collaboration-1435/cloud/tomato-cloud.exe';server_hash=digest(server)
assert server_hash=='BA11DEA650B92B40CF916D940E4B4BB62BEAD83C03A2E9C684411BAF6BF898CB'
evidence=candidate/'validation';evidence.mkdir(exist_ok=False)
for name,path in proofs.items():shutil.copy2(path,evidence/(name+'.json'))
for label in ['1494','1495']:
 directory=root/f'target/window-probes/table-sql-cloud-{label}'
 proof=load(directory/'sql_repeat_close.state.json')
 assert proof['status']=='passed' and proof['closed_saved_query'] and proof['error'] is None
 shutil.copy2(directory/'sql_repeat_close.state.json',evidence/f'cloud-close-functional-{label}.json')
 shutil.copy2(directory/'tomato-cloud-fixture-sql_repeat_close/application.log',evidence/f'cloud-close-capture-{label}.log')
shutil.copy2(root/'target/checkpoints/table-sql-1493/production-sources.json',evidence/'production-sources.json')
for shot in data['native-table']['captures']:
 shutil.copy2(shot['path'],evidence/('table-'+shot['state']+'.png'))
info={'status':'passed','scope':'Plain executed table SELECT and selectable read-only SQL preview',
 'source_freeze':'table-sql-1493','production_source_files':len(manifest),
 'client':{'path':'tomato.exe','bytes':exe.stat().st_size,'sha256':fingerprint},
 'server':{'path':'cloud/tomato-cloud.exe','sha256':server_hash,'bytes':server.stat().st_size,'unchanged_from':'collaboration-1435'},
 'previous_package':'data-import-1485; its unchanged Cloud/import payload and prior evidence are retained',
 'legacy_configuration':{'protected_files_unchanged':len(protected),'included_in_package':False},
 'validation':{'rust_workspace':{'passed':passed,'failed':failed,'ignored':ignored},'strict_clippy':True,'formatting':True,'cli_self_check':True,
  'database_fixture_cases':22,'native_table_cases':6,'native_cloud_cases':2,'native_table_and_cloud_exit_codes_zero':True,
  'table_layout':'Toolbar/grid pixels match baseline in plain/selected states; only the 21-pixel SQL footer changes',
  'cloud_close_capture_limit':'The save/repeat/last-tab-close workflow reported functional success and normal exit on automatic and Vulkan backends, but neither returned its framebuffer. Both diagnostics are retained. The two complete native capture cases cover connection attachment and query execution/save instead.',
  'clipboard':'Real egui selection/copy events assert OutputCommand::CopyText without writing the system clipboard',
  'saved_hosts_contacted':False,'memory':data['memory'],'embedded_adapters':data['embedded-adapters']['checks']},
 'limitations':['Streaming export/transfer retain their compatibility projections',
  'Idle memory is measured with isolated empty profiles; it does not bound active sessions',
  'Self-hosted Cloud deployment requirements remain documented in cloud/README.md']}
(candidate/'build-info.json').write_text(json.dumps(info,ensure_ascii=False,indent=2),encoding='utf-8')
(candidate/'SHA256SUMS.txt').write_text(f'{fingerprint}  tomato.exe\n{server_hash}  cloud/tomato-cloud.exe\n',encoding='utf-8')
print(json.dumps({'status':'passed','client_sha256':fingerprint,'tests':passed,'protected':len(protected)}))
