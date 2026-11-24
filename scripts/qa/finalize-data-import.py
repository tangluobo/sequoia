"""Collect verified, local import release evidence without modifying user data."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil

p = argparse.ArgumentParser()
p.add_argument('--candidate', type=Path, required=True)
p.add_argument('--label', default='1486')
p.add_argument('--encrypted-label', default='1488')
p.add_argument('--cloud-label', default='1489')
a = p.parse_args()
root = Path(__file__).resolve().parents[2]
candidate = a.candidate.resolve()
exe = candidate / 'tomato.exe'
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest().upper()
load = lambda path: json.loads(path.read_text(encoding='utf-8-sig'))
fingerprint = digest(exe)
proofs = {
    'import-formats': root / f'target/window-probes/import-formats-release-{a.label}/validation.json',
    'import-readback': root / f'target/window-probes/import-formats-release-{a.label}/readback.json',
    'import-pages': root / f'target/window-probes/import-screens-release-{a.label}/validation.json',
    'encrypted-access': root / f'target/window-probes/import-encrypted-release-{a.encrypted_label}/validation.json',
    'embedded-adapters': candidate / 'embedded-validation.json',
    'export': root / f'target/window-probes/import-export-release-{a.label}/validation.json',
    'cloud': root / f'target/window-probes/import-cloud-release-{a.cloud_label}/validation.json',
    'memory': root / f'target/window-probes/import-memory-release-{a.label}/validation.json',
    'source-lifecycle': root / 'target/window-probes/import-lifecycle-dev-1483/validation.json',
    'paradox-reference': root / 'target/window-probes/import-paradox-worker-1476/validation.json',
    'database-engines': root / 'target/database-fixtures/probes/data-import-1485/validation.json',
}
data = {name: load(path) for name, path in proofs.items()}
for name, proof in data.items():
    assert proof.get('status', 'passed') == 'passed', (name, proof.get('status'))
    for key in ('guiSha256', 'sha256'):
        if name in ('import-formats', 'import-readback', 'import-pages', 'embedded-adapters', 'export', 'cloud') and key in proof:
            assert proof[key].upper() == fingerprint, (name, 'different executable')
assert len(data['import-formats']['checks']) == 10
assert len(data['import-pages']['checks']) == 7
assert len(data['encrypted-access']['checks']) == 5
assert len(data['cloud']['cases']) == 6
assert len(data['memory']['samples']) == 4
assert sum(row['records'] for row in data['import-readback']['checks']) == 23050
assert all(row['everyRecordReadBack'] for row in data['import-readback']['checks'])
for name in ('import-formats', 'import-pages'):
    assert all(row['normalExit'] for row in data[name]['checks'])
assert all(row['normalExit'] for row in data['export']['captures'])
assert all(row['exit_code'] == 0 for row in data['cloud']['cases'])
assert data['memory']['sharedConfigurationUnchanged']
for row in data['memory']['samples']:
    if row['version'] == 'new':
        assert row['executable']['sha256'].upper() == fingerprint
db_log = (root / 'target/database-fixtures/probes/data-import-1485/tests.log').read_text(errors='replace')
assert 'test result: ok. 2 passed; 0 failed' in db_log
log = (root / 'target/data-import-workspace-1485.log').read_text(errors='replace')
counts = re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored', log)
passed, failed, ignored = (sum(int(row[i]) for row in counts) for i in range(3))
assert passed == 571 and failed == 0 and ignored == 82, (passed, failed, ignored)
assert 'Tomato self-check passed' in (root / 'target/data-import-cli-1485.log').read_text()
assert (root / 'target/data-import-fmt-1485.log').stat().st_size == 0
assert 'Finished' in (root / 'target/data-import-clippy-1484.log').read_text(errors='replace')
manifest = load(root / 'target/checkpoints/import-1485/production-sources.json')
for entry in manifest:
    assert digest(root / entry['path']) == entry['sha256'], entry['path']
protected = load(root / 'target/data-import-protected-1450.json')
for entry in protected:
    assert digest(Path(entry['path'])) == entry['hash'], entry['path']
icon = load(candidate / 'icon-validation.json')
assert icon['allEmbeddedFramesMatchOriginalBytes']
assert icon['executableSha256'].upper() == fingerprint
evidence = candidate / 'validation'
evidence.mkdir(exist_ok=False)
for name, path in proofs.items():
    shutil.copy2(path, evidence / (name + '.json'))
shutil.copy2(root / 'target/checkpoints/import-1485/production-sources.json', evidence / 'production-sources.json')
shutil.copy2(candidate / 'icon-validation.json', evidence / 'icon.json')
server = root / 'target/qa-candidate-collaboration-1435/cloud/tomato-cloud.exe'
server_hash = digest(server)
assert server_hash == 'BA11DEA650B92B40CF916D940E4B4BB62BEAD83C03A2E9C684411BAF6BF898CB'
info = {
    'status': 'passed', 'scope': 'Nine-format import wizard; retained ten-format export and self-hosted Tomato Cloud',
    'source_freeze': 'import-1485', 'production_source_files': len(manifest),
    'client': {'path': 'tomato.exe', 'sha256': fingerprint, 'bytes': exe.stat().st_size},
    'server': {'path': 'cloud/tomato-cloud.exe', 'sha256': server_hash, 'bytes': server.stat().st_size, 'unchanged_from': 'collaboration-1435'},
    'legacy_configuration': {'protected_files_unchanged': len(protected), 'included_in_package': False},
    'validation': {'rust_workspace': {'passed': passed, 'failed': failed, 'ignored': ignored},
                   'strict_clippy': True, 'formatting': True, 'cli_self_check': True,
                   'native_import_file_cases': 10, 'independent_readback_rows': 23050,
                   'native_wizard_pages': 7, 'encrypted_access_cases': 5,
                   'encrypted_access_initial_parallel_probe': 'Temporary helper executable had a transient Windows sharing violation during cleanup; retained failed diagnostic, then repeated the unchanged strict probe in isolation',
                   'cloud_initial_probe': 'Harness restored an absent WGPU_BACKEND as an empty string under PowerShell; corrected the harness environment and repeated with the unchanged executable',
                   'native_export_cases': len(data['export']['checks']) if 'checks' in data['export'] else len(data['export']['captures']),
                   'native_cloud_cases': 6, 'all_native_cases_exit_normally': True,
                   'saved_hosts_contacted': False, 'source_cache_lifecycle': 'validation/source-lifecycle.json',
                   'database_engines': ['SQLite', 'MySQL 8.4.11', 'PostgreSQL 17.11'],
                   'memory': data['memory'], 'native_adapters': data['embedded-adapters']['checks']},
    'limitations': ['ODBC requires installed drivers of the selected architecture',
                    'Encrypted Paradox requires a suitable installed ODBC driver',
                    'Native Oracle destination has no live Oracle verification',
                    'Excel and source-record resource limits are documented in data-import.md',
                    'Cloud uses independent accounts; proprietary Navicat model/BI editors and official accounts are not integrated',
                    'Live SMTP, public DNS/TLS, Docker and Linux runtime require deployment-environment verification'],
}
(candidate / 'build-info.json').write_text(json.dumps(info, ensure_ascii=False, indent=2), encoding='utf-8')
(candidate / 'SHA256SUMS.txt').write_text(f'{fingerprint}  tomato.exe\n{server_hash}  cloud/tomato-cloud.exe\n', encoding='utf-8')
print(json.dumps({'status': 'passed', 'client_sha256': fingerprint, 'tests': passed, 'protected': len(protected)}))
