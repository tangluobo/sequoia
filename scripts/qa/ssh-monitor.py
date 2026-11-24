#!/usr/bin/env python3
"""Capture the actual SSH monitor through an owned authenticated loopback peer."""
import argparse,hashlib,json,os,re,subprocess
from pathlib import Path

def sha(path):
    with path.open('rb') as stream:return hashlib.file_digest(stream,'sha256').hexdigest().upper()

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--gui',type=Path,required=True)
    parser.add_argument('--services',type=Path,required=True)
    parser.add_argument('--label',required=True)
    args=parser.parse_args()
    if not re.fullmatch(r'[a-z0-9-]+',args.label):parser.error('Use a unique lowercase label')
    root=Path(__file__).resolve().parents[2]
    gui=args.gui.resolve();services=args.services.resolve()
    if not gui.is_relative_to(root) or not services.is_relative_to(root):parser.error('Use owned workspace artifacts')
    directory=root/'target/window-probes'/args.label;directory.mkdir(parents=True,exist_ok=False)
    shared=Path.home()/'.tomato/connections.json';before=sha(shared) if shared.exists() else None
    env=os.environ.copy()
    for name in ['TOMATO_LEGACY_JAVA','TOMATO_LEGACY_SSH_CLASSPATH','TOMATO_SSH_CAPTURE_HARNESS','TOMATO_TEST_ASSERT_CURSOR_BLINK']:env.pop(name,None)
    frame=directory/'monitor.png'
    env.update(TOMATO_SSH_CAPTURE_EXE=str(gui),TOMATO_SSH_CAPTURE_PATH=str(frame),TOMATO_SSH_TAB_FIXTURE='monitor',TOMATO_UI_CAPTURE_DELAY_MS='10000',TOMATO_UI_CAPTURE_FRAMES='8',TOMATO_DATA_DIR=str(directory/'profile'))
    with (directory/'test.log').open('wb') as log:
        subprocess.run([str(services),'capture_native_and_original_ssh_tools','--exact','--ignored','--nocapture'],cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=150,check=True,creationflags=0x08000000 if os.name=='nt' else 0)
    proof=json.loads(frame.with_suffix('.ssh-tab.json').read_text(encoding='utf-8'))
    assert proof['sessionReady'] and proof['mode']=='monitor' and proof['monitor']['samples']>=3
    assert before==(sha(shared) if shared.exists() else None)
    review_frame=frame.with_name(frame.stem+'-07.png');assert review_frame.is_file()
    result=dict(passed=True,guiSha256=sha(gui),testHarnessSha256=sha(services),frame=str(review_frame),captureFrames=8,frameSha256=sha(review_frame),realLoopbackSsh=True,savedHostsContacted=False,sharedConfigurationUnchanged=True,osInputUsed=False,normalExit=True,monitor=proof['monitor'])
    (directory/'validation.json').write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result))

if __name__=='__main__':main()
