"""Actual owned JSON/MQ windows: select, scroll and intercept clipboard output."""
from pathlib import Path
import argparse
import hashlib
import json
import os
import re
import subprocess

ROOT=Path(__file__).resolve().parents[2]
def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest().upper()
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--exe',required=True);parser.add_argument('--label',required=True);parser.add_argument('--reference',required=True)
    args=parser.parse_args();assert re.fullmatch(r'[a-z0-9-]{1,80}',args.label)
    program=Path(args.exe).resolve(strict=True);reference=Path(args.reference).resolve(strict=True)
    assert program.name=='tomato.exe'and(program.is_relative_to(ROOT/'target')or program.is_relative_to(ROOT/'dist'))
    assert reference.is_relative_to(ROOT/'target')
    original=json.loads((reference/'validation.json').read_text(encoding='utf-8'));assert original['passed']and not original['systemClipboardWritten']
    assert all(sha(Path(e['path']))==e['sha256']for e in original['sources'])
    assert all(sha(Path(p))==h for p,h in original.get('fixtureSources',{}).items())
    output=ROOT/'target/window-probes'/args.label;output.mkdir(parents=True,exist_ok=False)
    identity=sha(program);results=[]
    for case in ['structured','folded','raw','mq','kafka','mq-folded','mq-raw','mq-scalar']:
        directory=output/case;directory.mkdir();profile=directory/('tomato-json-selection-fixture-'+case);profile.mkdir()
        source=reference/'tomato_json_sources'/('raw.txt'if case=='raw'else'mq-raw.txt'if case=='mq-raw'else'mq-scalar.txt'if case=='mq-scalar'else'mq.json'if case in ['mq','kafka','mq-folded']else'structured.json')
        (profile/'input.txt').write_bytes(source.read_bytes())
        config=profile/'connections.json';config.write_text(json.dumps(dict(connections=[],groups=[],unknownOwnedField={'preserve':True})),encoding='utf-8');before=sha(config)
        capture=directory/'capture.png'
        env={k:v for k,v in os.environ.items()if not k.startswith('TOMATO_')}
        env.update(TOMATO_DATA_DIR=str(profile),TOMATO_UI_CAPTURE=str(capture),TOMATO_UI_CAPTURE_FRAMES='2',TOMATO_UI_PREVIEW='json_selection',TOMATO_JSON_SELECTION_CASE=case,WGPU_BACKEND='vulkan')
        startup=subprocess.STARTUPINFO();startup.dwFlags=subprocess.STARTF_USESHOWWINDOW;startup.wShowWindow=0
        with(directory/'stdout.log').open('wb')as out,(directory/'stderr.log').open('wb')as err:
            subprocess.run([str(program)],cwd=ROOT,env=env,stdout=out,stderr=err,timeout=120,check=True,creationflags=0x08000000,startupinfo=startup)
        proof=json.loads(capture.with_suffix('.selection.json').read_text(encoding='utf-8'));assert proof['copies']==(3 if case=='raw'else 2)
        expected=(reference/('mq-selected.txt'if case=='kafka'else case+'-selected.txt')).read_bytes().decode('utf-8')
        original_case='mq'if case=='kafka'else case
        assert sha(reference/(original_case+'-selected.txt'))==next(c['visibleSha256']for c in original['cases']if c['case']==original_case)
        actual=capture.with_suffix('.copy-0.txt').read_bytes().decode('utf-8');assert actual==expected,(case,'Actual selected text differs from original')
        if case=='raw':assert capture.with_suffix('.copy-1.txt').read_bytes().decode('utf-8')==expected[-1:]
        toolbar=capture.with_suffix('.copy-2.txt'if case=='raw'else'.copy-1.txt').read_bytes().decode('utf-8');assert toolbar==expected,(case,'Toolbar copy differs from selected text')
        assert sha(config)==before and not proof['details']['error']and proof['details']['longLineCacheEntries']<=128
        assert proof['details']['lineHeight']==(19 if case=='raw'else 14),(case,'Paragraph spacing differs from original')
        assert proof['details']['gutter']==(5 if case=='raw'else 70)
        assert not proof['systemClipboardWritten']and not proof['physicalOsInputUsed']and not proof['savedHostsContacted']
        if case=='raw':assert proof['maxReadonlyShapedBytes']<=4096
        frame=directory/'capture-01.png';assert frame.is_file()
        results.append(dict(case=case,normalExit=True,selectedTextMatchesOriginal=True,selectedBytes=len(actual.encode('utf-8')),capture=str(frame),captureSha256=sha(frame),proof=proof))
        print(case+' native copy and normalexit passed',flush=True)
    assert sha(program)==identity and all(sha(Path(e['path']))==e['sha256']for e in original['sources'])
    proof=dict(passed=True,guiSha256=identity,cases=results,configurationUnchanged=True,ownedFixturesOnly=True,physicalOsInputUsed=False,systemClipboardWritten=False,savedHostsContacted=False,fullJavaVisualParity=False)
    (output/'validation.json').write_text(json.dumps(proof,ensure_ascii=False,indent=2),encoding='utf-8')
if __name__=='__main__':main()
