"""Verify the release contains exactly one copy of each reviewed native adapter."""
import argparse,hashlib,json,pathlib
p=argparse.ArgumentParser();p.add_argument('exe',type=pathlib.Path);p.add_argument('output',type=pathlib.Path);a=p.parse_args();root=pathlib.Path(__file__).resolve().parents[2];image=a.exe.read_bytes();checks=[]
for relative in ['vendor/access-export/bin/x86_64-pc-windows-msvc/tomato-access.exe','vendor/odbc-import/bin/i686-pc-windows-msvc/tomato-odbc.exe']:
 blob=(root/relative).read_bytes();offset=image.find(blob);assert offset>=0,relative;assert image.find(blob,offset+1)<0,relative+' duplicated'
 checks.append({'adapter':relative,'offset':offset,'bytes':len(blob),'sha256':hashlib.sha256(blob).hexdigest().upper(),'copies':1})
result={'status':'passed','guiSha256':hashlib.sha256(image).hexdigest().upper(),'checks':checks};a.output.write_text(json.dumps(result,indent=2),encoding='utf-8');print(json.dumps(result))
