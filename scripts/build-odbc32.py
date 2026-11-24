"""Build the small, static-CRT Windows x86 ODBC reader; no driver is redistributed."""
import hashlib,json,os,pathlib,subprocess
root=pathlib.Path(__file__).resolve().parents[1]
vswhere=pathlib.Path(os.environ.get('ProgramFiles(x86)','C:/Program Files (x86)'))/'Microsoft Visual Studio/Installer/vswhere.exe'
install=subprocess.check_output([str(vswhere),'-latest','-products','*','-requires','Microsoft.VisualStudio.Component.VC.Tools.x86.x64','-property','installationPath'],text=True).strip()
if not install:raise SystemExit('Visual C++ x86 build tools are required')
vcvars=pathlib.Path(install)/'VC/Auxiliary/Build/vcvarsall.bat'
out=root/'vendor/odbc-import/bin/i686-pc-windows-msvc';out.mkdir(parents=True,exist_ok=True)
obj=root/'target/odbc32-build';obj.mkdir(parents=True,exist_ok=True)
source=root/'vendor/odbc-import/odbc.c';binary=out/'tomato-odbc.exe'
batch=obj/'build.cmd'
batch.write_text(f'@echo off\ncall "{vcvars}" x86 >nul\nif errorlevel 1 exit /b 1\ncl.exe /nologo /O2 /MT /W4 /DUNICODE /D_UNICODE /D_CRT_SECURE_NO_WARNINGS "{source}" /Fo"{obj / "odbc.obj"}" /Fe"{binary}" /link odbc32.lib\nexit /b %errorlevel%\n',encoding='utf-8')
subprocess.run([os.environ.get('COMSPEC','cmd.exe'),'/d','/c','call',str(batch)],check=True,cwd=obj)
(out/'build-info.json').write_text(json.dumps({'target':'i686-pc-windows-msvc','sourceSha256':hashlib.sha256(source.read_bytes()).hexdigest(),'binarySha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'runtime':'static CRT; Windows ODBC driver manager'},indent=2),encoding='utf-8')
print(binary)
