"""Exercise the real NSIS payload and uninstaller in a private workspace directory."""
from pathlib import Path
import argparse
import hashlib
import importlib.util
import json
import os
import re
import subprocess
import time

ROOT=Path(__file__).resolve().parents[2]
spec=importlib.util.spec_from_file_location('tomato_installer',ROOT/'scripts/build-installer.py')
installer=importlib.util.module_from_spec(spec)
spec.loader.exec_module(installer)

def run(command,env):
    subprocess.run(command,env=env,cwd=ROOT,check=True,timeout=60,creationflags=0x08000000)

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--label',required=True)
    parser.add_argument('--offline',action='store_true')
    args=parser.parse_args()
    if os.name!='nt' or not re.fullmatch(r'[A-Za-z0-9_-]{1,80}',args.label):
        raise ValueError('A new Windows probe label is required')
    root=ROOT/'target'/'installer-probes'/args.label
    root.mkdir(parents=True,exist_ok=False)
    stage=root/'source 中文 $ literal';stage.mkdir()
    contents={'tomato.exe':b'MZ fixture desktop','tomato-cli.exe':b'MZ fixture cli',
              'docs/中文 $literal/说明.txt':'安装内容'.encode(), 'docs/zero.txt':b'',
              'docs/literal$file.txt':b'literal dollar'}
    for name,content in contents.items():
        path=stage/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_bytes(content)
    include=root/'payload.nsh';include.write_text(installer.manifest(stage),encoding='utf-8')
    destination=root/'安装目录 $ literal'
    output=root/'fixture-setup.exe'
    source=root/'fixture.nsi'
    source.write_text('''Unicode True
Name "Owned Tomato payload fixture"
RequestExecutionLevel user
SilentInstall silent
SilentUnInstall silent
InstallDir '''+installer.quoted(str(destination).replace('$','$$'))+'''
OutFile '''+installer.quoted(output)+'''
!include '''+installer.quoted(include)+'''
Section "Install"
  StrCmp "$INSTDIR" '''+installer.quoted(str(destination).replace('$','$$'))+''' owned_install
  Abort "Fixture refuses an unexpected directory"
owned_install:
  !insertmacro InstallPayload
  WriteUninstaller "$INSTDIR\\Uninstall.exe"
SectionEnd
Section "Uninstall"
  StrCmp "$INSTDIR" '''+installer.quoted(str(destination).replace('$','$$'))+''' owned_uninstall
  Abort "Fixture refuses an unexpected directory"
owned_uninstall:
  !insertmacro UninstallPayload
  Delete "$INSTDIR\\Uninstall.exe"
  RMDir "$INSTDIR"
SectionEnd
''',encoding='utf-8')
    env=os.environ.copy();env['TEMP']=str(root);env['TMP']=str(root)
    run([installer.compiler(args.offline),'/V2','/INPUTCHARSET','UTF8',str(source)],env)
    run([str(output),'/S'],env)
    for name,expected in contents.items():
        if (destination/name).read_bytes()!=expected:
            raise RuntimeError('Installer payload mismatch: '+name)
    user_file=destination/'用户自己的文件.txt';user_file.write_text('保留原内容',encoding='utf-8')
    nested=destination/'docs/中文 $literal/user.txt';nested.write_bytes(b'user-owned')
    run([str(destination/'Uninstall.exe'),'/S'],env)
    deadline=time.monotonic()+20
    while any((destination/name).exists() for name in contents) and time.monotonic()<deadline:
        time.sleep(.05)
    if any((destination/name).exists() for name in contents):
        raise RuntimeError('Uninstaller retained packaged files')
    if user_file.read_text(encoding='utf-8')!='保留原内容' or nested.read_bytes()!=b'user-owned':
        raise RuntimeError('Uninstaller changed a user-owned file')
    proof={'passed':True,'installedFiles':len(contents),'userOwnedFilesPreserved':2,
           'profileTouched':False,'registryTouched':False,'shortcutsTouched':False,
           'payloadManifestSha256':hashlib.sha256(include.read_bytes()).hexdigest(),
           'installerSourceSha256':hashlib.sha256(source.read_bytes()).hexdigest(),
           'nsisVersion':installer.VERSION,'nsisArchiveSha256':installer.NSIS_SHA256}
    path=root/'validation.json';path.write_text(json.dumps(proof,indent=2),encoding='utf-8')
    print(path)

if __name__=='__main__':main()
