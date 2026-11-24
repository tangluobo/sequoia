"""Bind filter/paging regression evidence to the exact release, without saved hosts."""
import argparse, hashlib, json, pathlib, re, shutil
p=argparse.ArgumentParser();p.add_argument('--candidate',type=pathlib.Path,required=True);p.add_argument('--label',required=True);a=p.parse_args()
root=pathlib.Path(__file__).resolve().parents[2];candidate=a.candidate.resolve();label=a.label
load=lambda p:json.loads(p.read_text(encoding='utf-8-sig'))
digest=lambda p:hashlib.sha256(p.read_bytes()).hexdigest().upper()
exe=candidate/'tomato.exe';fingerprint=digest(exe)
proofs={
 'native-table':root/f'target/window-probes/table-filter-release-{label}/validation.json',
 'table-layout':root/f'target/window-probes/table-filter-release-{label}/layout-validation.json',
 'native-cloud':root/f'target/window-probes/table-filter-cloud-{label}/validation.json',
 'memory':root/f'target/window-probes/table-filter-memory-{label}/validation.json',
 'database-engines':root/f'target/database-fixtures/probes/table-filter-{label}/validation.json',
 'embedded-adapters':candidate/'embedded-validation.json','icon':candidate/'icon-validation.json',
}
data={name:load(path) for name,path in proofs.items()}
for name,proof in data.items():
 assert proof.get('status','passed')=='passed',name
 if name in ('native-table','table-layout','native-cloud','embedded-adapters'):
  assert (proof.get('guiSha256') or proof.get('sha256')).upper()==fingerprint,name
assert len(data['native-table']['captures'])==8
assert len(data['table-layout']['checks'])==2 and all(row['toolbarAndGridPixelIdentical'] for row in data['table-layout']['checks'])
assert not data['native-table']['savedHostsContacted'] and data['native-table']['sharedConfigurationUnchanged']
assert len(data['native-cloud']['cases'])==2 and all(row['exit_code']==0 for row in data['native-cloud']['cases'])
assert data['icon']['allEmbeddedFramesMatchOriginalBytes'] and data['icon']['executableSha256']==fingerprint
assert data['memory']['sharedConfigurationUnchanged'] and len(data['memory']['samples'])==4
for sample in data['memory']['samples']:
 if sample['version']=='new':assert sample['executable']['sha256']==fingerprint
workspace=(root/f'target/table-filter-workspace-{label}.log').read_text(errors='replace')
counts=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored',workspace)
passed,failed,ignored=(sum(int(row[i]) for row in counts) for i in range(3))
assert (passed,failed,ignored)==(577,0,82),(passed,failed,ignored)
for name in ['table_filter_sort_count_paging_and_export_execute_on_the_database','unresolved_postgres_type_declarations_do_not_panic_or_change_text_to_boolean','sql_preview_keyboard_selection_copies_and_cannot_edit_the_statement','sql_preview_context_menu_copies_full_query_even_when_showing_a_status_message','table_menu_filters_and_multisort_dispatch_real_database_queries']:
 assert name+' ... ok' in workspace,name
assert 'Finished' in (root/f'target/table-filter-clippy-{label}.log').read_text(errors='replace')
assert (root/f'target/table-filter-fmt-{label}.log').stat().st_size==0
assert 'Tomato self-check passed' in (root/f'target/table-filter-cli-{label}.log').read_text()
assert 'test result: ok. 22 passed; 0 failed' in (root/f'target/database-fixtures/probes/table-filter-{label}/tests.log').read_text(errors='replace')
manifest=load(root/f'target/checkpoints/table-filter-{label}/production-sources.json')
for entry in manifest:assert digest(root/entry['path'])==entry['sha256'],entry['path']
protected=load(root/'target/table-filter-protected-1497.json')
for entry in protected:assert digest(pathlib.Path(entry['path']))==entry['hash'],entry['path']
server=root/'target/qa-candidate-collaboration-1435/cloud/tomato-cloud.exe';server_hash=digest(server)
assert server_hash=='BA11DEA650B92B40CF916D940E4B4BB62BEAD83C03A2E9C684411BAF6BF898CB'
evidence=candidate/'validation';evidence.mkdir(exist_ok=False)
for name,path in proofs.items():shutil.copy2(path,evidence/(name+'.json'))
shutil.copy2(root/f'target/checkpoints/table-filter-{label}/production-sources.json',evidence/'production-sources.json')
for name in ['workspace','clippy','fmt','cli']:shutil.copy2(root/f'target/table-filter-{name}-{label}.log',evidence/(name+'.log'))
shutil.copy2(root/f'target/table-filter-workspace-interrupted-{label}.log',evidence/'parallel-ui-attempt-interrupted.log')
for name in ['stdout','stderr']:shutil.copy2(root/f'target/window-probes/table-config-isolated-{label}/{name}.log',evidence/f'isolated-terminal-config-{name}.log')
for shot in data['native-table']['captures']:shutil.copy2(shot['path'],evidence/('table-'+shot['state']+'.png'))
info={'status':'passed','scope':'Database-side table filtering, multi-column sorting, count/pagination/export and captured executed SQL',
 'source_freeze':f'table-filter-{label}','production_source_files':len(manifest),
 'client':{'path':'tomato.exe','bytes':exe.stat().st_size,'sha256':fingerprint},
 'server':{'path':'cloud/tomato-cloud.exe','sha256':server_hash,'bytes':server.stat().st_size,'unchanged_from':'collaboration-1435'},
 'previous_package':'table-sql-1493; unchanged Cloud/import/export payload retained',
 'legacy_configuration':{'protected_files_unchanged':len(protected),'included_in_package':False},
 'validation':{'rust_workspace':{'passed':passed,'failed':failed,'ignored':ignored,'test_threads':1},'strict_clippy':True,'formatting':True,'cli_self_check':True,
  'database_fixture_cases':22,'native_table_cases':8,'native_cloud_cases':2,'native_exit_codes_zero':True,'saved_hosts_contacted':False,
  'clipboard':'Real egui copy output, without writing the system clipboard','memory':data['memory'],'embedded_adapters':data['embedded-adapters']['checks'],
  'windows_console_fixture':'An initial parallel UI suite stalled in the existing terminal configuration copy fixture and was interrupted. Its isolated rerun passed in 0.14s (application drop 44ms); the complete workspace then passed serially. Diagnostics are retained. Windows CI/release tests use one test thread; implementation sources are unchanged by that test-mode adjustment.'},
 'limitations':['Oracle generation connected but not verified against a live Oracle server',
  'Two statements count and fetch; concurrent database writes can change the result between them',
  'Tables without unique keys cannot guarantee tie ordering under concurrent writes',
  'Contains/quick-search predicates can require text conversions and table scans',
  'Nested criteria groups and raw SQL-expression builder are not included; use the query editor',
  'Idle memory measurements do not bound active sessions']}
(candidate/'build-info.json').write_text(json.dumps(info,ensure_ascii=False,indent=2),encoding='utf-8')
(candidate/'SHA256SUMS.txt').write_text(f'{fingerprint}  tomato.exe\n{server_hash}  cloud/tomato-cloud.exe\n',encoding='utf-8')
print(json.dumps({'status':'passed','client_sha256':fingerprint,'tests':passed,'protected':len(protected)}))
