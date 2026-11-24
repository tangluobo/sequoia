"""Build a synthetic, loopback-only independent RDP peer in the workspace.

This helper never installs FreeRDP and never starts its desktop-sharing servers.
The sample server only draws its bundled test image and background.
"""
from pathlib import Path
import argparse, hashlib, json, os, shutil, subprocess, tarfile, urllib.request

ROOT=Path(__file__).resolve().parents[2]
SOURCES=[
 ('freerdp-3.32.2.tar.gz','https://pub.freerdp.com/releases/freerdp-3.32.2.tar.gz','e95a505c0057e4d06a3b120477c421028769deba4db9c639ef3465d050f0955c'),
 ('openssl-3.6.5.pkg.tar.zst','https://repo.msys2.org/mingw/ucrt64/mingw-w64-ucrt-x86_64-openssl-3.6.5-1-any.pkg.tar.zst','773e021ebad83f14b5c103db467411e8ff064663e1c7e7f730e2be72f4117064'),
]
def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake',required=True,type=Path)
    parser.add_argument('--ninja',required=True,type=Path)
    parser.add_argument('--toolchain',required=True,type=Path)
    parser.add_argument('--prepare-only',action='store_true')
    args=parser.parse_args()
    output=ROOT/'target/freerdp-fixture-1672';output.mkdir(exist_ok=True)
    prefix=output/'sdk';source=output/'source';source.mkdir(exist_ok=True);prefix.mkdir(exist_ok=True)
    for name,url,digest in SOURCES:
        archive=output/name
        content=archive.read_bytes() if archive.exists() else urllib.request.urlopen(url,timeout=60).read(64*1024*1024+1)
        if len(content)>64*1024*1024 or hashlib.sha256(content).hexdigest()!=digest:raise ValueError('Fixture archive checksum mismatch: '+name)
        if not archive.exists():archive.write_bytes(content)
        destination=source if name.startswith('freerdp') else prefix
        with tarfile.open(archive) as stream:
            for item in stream:
                if not item.isfile():continue
                path=Path(item.name)
                if path.is_absolute() or '..' in path.parts or item.size>32*1024*1024:raise ValueError('Unexpected archive member')
                target=destination/path
                if target.is_file():continue
                target.parent.mkdir(exist_ok=True,parents=True)
                with stream.extractfile(item) as reader:target.write_bytes(reader.read(item.size+1))
    project=source/'freerdp-3.32.2'
    sample=project/'server/Sample/sfreerdp.c'
    text=sample.read_text(encoding='utf-8')
    before='instance->Open(instance, nullptr, (UINT16)port)'
    after='instance->Open(instance, "127.0.0.1", (UINT16)port)'
    if before in text:sample.write_text(text.replace(before,after),encoding='utf-8')
    elif after not in text:raise ValueError('Loopback-only sample patch did not match')
    text=sample.read_text(encoding='utf-8')
    text=text.replace('freerdp_settings_set_bool(settings, FreeRDP_TlsSecurity, TRUE)','freerdp_settings_set_bool(settings, FreeRDP_TlsSecurity, FALSE)')
    marker='\t/*  ENCRYPTION_LEVEL_HIGH; */'
    if 'TOMATO_RDP_FIXTURE_STRENGTH' not in text:
        if marker not in text:raise ValueError('Sample protection configuration did not match')
        settings='''
    /* Tomato QA: choose documented server settings, never change cryptography. */
    {
        const char* strength = getenv("TOMATO_RDP_FIXTURE_STRENGTH");
        UINT32 methods = ENCRYPTION_METHOD_128BIT;
        UINT32 level = ENCRYPTION_LEVEL_CLIENT_COMPATIBLE;
        if (strength && strcmp(strength, "56") == 0) methods = ENCRYPTION_METHOD_56BIT;
        else if (strength && strcmp(strength, "40") == 0) methods = ENCRYPTION_METHOD_40BIT;
        else if (strength && strcmp(strength, "low") == 0) level = ENCRYPTION_LEVEL_LOW;
        else if (strength && strcmp(strength, "high") == 0) level = ENCRYPTION_LEVEL_HIGH;
        else if (strength && strcmp(strength, "none") == 0) { methods = 0; level = ENCRYPTION_LEVEL_NONE; }
        if (!freerdp_settings_set_uint32(settings, FreeRDP_EncryptionMethods, methods) ||
            !freerdp_settings_set_uint32(settings, FreeRDP_EncryptionLevel, level)) goto fail;
    }
'''
        text=text.replace(marker,settings+marker)
    if 'Tomato QA: do not bypass encryption for a loopback TCP peer.' not in text:
        marker='\tclient->ContextExtra = info;'
        if marker not in text:raise ValueError('Sample local-transport configuration did not match')
        text=text.replace(marker,marker+'''
    /* Tomato QA: do not bypass encryption for a loopback TCP peer. */
    {
        const char* strength = getenv("TOMATO_RDP_FIXTURE_STRENGTH");
        if (!strength || strcmp(strength, "none") != 0) client->local = FALSE;
    }
''')
    sample.write_text(text,encoding='utf-8')
    images=output/'install/share/FreeRDP/images';images.mkdir(parents=True,exist_ok=True)
    for name in ['test_icon.bmp','test_icon.png','test_icon.jpg','test_icon.webp']:
        shutil.copyfile(project/'server/Sample'/name,images/name)
    proof={'syntheticDesktopOnly':True,'bind':'127.0.0.1','systemInstalled':False,'sourceArchives':[{'url':u,'sha256':h}for _,u,h in SOURCES], 'samplePatch':'Bind sample TCP listener to IPv4 loopback, disable TLS, select documented encryption methods/levels and disable the local-peer encryption bypass for encrypted QA modes; FreeRDP protocol and crypto implementation are unmodified'}
    (output/'source.json').write_text(json.dumps(proof,indent=2),encoding='utf-8')
    if args.prepare_only:return
    build=output/'build';env=os.environ.copy();env['PATH']=str(args.toolchain)+os.pathsep+env['PATH']
    command=[str(args.cmake),'-S',str(project),'-B',str(build),'-G','Ninja',
       '-DCMAKE_BUILD_TYPE=Release','-DCMAKE_INSTALL_PREFIX='+str(output/'install'),'-DCMAKE_C_FLAGS=-D__STDC_NO_THREADS__=1 -Wno-error=incompatible-pointer-types','-DCMAKE_C_COMPILER='+str(args.toolchain/'gcc.exe'),
       '-DCMAKE_CXX_COMPILER='+str(args.toolchain/'g++.exe'),'-DCMAKE_MAKE_PROGRAM='+str(args.ninja),
       '-DCMAKE_PREFIX_PATH='+str(prefix/'ucrt64'),'-DOPENSSL_ROOT_DIR='+str(prefix/'ucrt64')]
    enabled=['WITH_SERVER','WITH_SAMPLE']
    disabled=['USE_VERSION_FROM_GIT_TAG','USE_GIT_FOR_REVISION','WITH_SMARTCARD_EMULATE','BUILD_TESTING','WITH_CLIENT','WITH_PLATFORM_SERVER','WITH_SHADOW','WITH_FFMPEG','WITH_DSP_FFMPEG','WITH_SDL','WITH_MEDIA_FOUNDATION','WITH_CAIRO','WITH_ZLIB','WITH_JPEG','WITH_WEBP','WITH_PULSE','WITH_ALSA','WITH_CUPS','WITH_WINPR_JSON','WITH_URIPARSER','WITH_PCSC','WITH_OPENH264','WITH_VERBOSE_WINPR_ASSERT']
    disabled+=['WITH_RESOURCE_VERSIONING','USE_UNWIND','WITH_CLIENT_CHANNELS','CHANNEL_URBDRC','CHANNEL_URBDRC_CLIENT','WITH_SWSCALE','WITH_VIDEO_FFMPEG','WITH_OPUS','WITH_GSM','WITH_LAME','WITH_FAAC','WITH_FAAD2','WITH_SOXR']
    command += ['-D'+name+'=ON'for name in enabled]+['-D'+name+'=OFF'for name in disabled]
    with (output/'configure.log').open('w',encoding='utf-8') as log:subprocess.run(command,env=env,cwd=ROOT,check=True,stdout=log,stderr=subprocess.STDOUT,creationflags=0x08000000)
    with (output/'build.log').open('w',encoding='utf-8') as log:subprocess.run([str(args.cmake),'--build',str(build),'--target','sfreerdp-server','--parallel','4'],env=env,cwd=ROOT,check=True,stdout=log,stderr=subprocess.STDOUT,creationflags=0x08000000)
    print(build/'server/Sample/sfreerdp-server.exe')
if __name__=='__main__':main()
