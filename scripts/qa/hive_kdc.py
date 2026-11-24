"""Own a loopback MIT realm, database, keytabs and FILE ticket cache for HS2 QA."""
from contextlib import contextmanager
from pathlib import Path
import os
import shutil
import socket
import subprocess
import tempfile
import time

@contextmanager
def realm(parent):
    if os.name!='posix':
        raise RuntimeError('Private FILE-cache Kerberos QA requires Unix')
    tools={name:shutil.which(name) or str(Path('/usr/sbin')/name) for name in ('kdb5_util','kadmin.local','krb5kdc','kinit')}
    for path in tools.values():
        if not Path(path).is_file():raise RuntimeError('MIT Kerberos fixture tool missing: '+path)
    with tempfile.TemporaryDirectory(prefix='owned-realm-',dir=parent) as temporary:
        root=Path(temporary)
        with socket.socket() as listener:
            listener.bind(('127.0.0.1',0));port=listener.getsockname()[1]
        config=root/'krb5.conf'
        config.write_text(f'''[libdefaults]
 default_realm = TOMATO.TEST
 dns_lookup_kdc = false
 dns_lookup_realm = false
 rdns = false
 udp_preference_limit = 1
[realms]
 TOMATO.TEST = {{
  kdc = 127.0.0.1:{port}
 }}
''')
        profile=root/'kdc.conf'
        profile.write_text(f'''[kdcdefaults]
 kdc_listen = 127.0.0.1:{port}
 kdc_tcp_listen = 127.0.0.1:{port}
[realms]
 TOMATO.TEST = {{
  database_name = {root}/principal
  key_stash_file = {root}/stash
  acl_file = {root}/acl
  max_life = 1h
 }}
[logging]
 kdc = FILE:{root}/kdc.log
''')
        (root/'acl').write_text('')
        env=os.environ.copy()
        env.update(KRB5_CONFIG=str(config),KRB5_KDC_PROFILE=str(profile),KRB5CCNAME='FILE:'+str(root/'tickets'),KRB5_KTNAME=str(root/'hive.keytab'))
        def run(arguments):
            subprocess.run(arguments,env=env,check=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=30)
        run([tools['kdb5_util'],'create','-s','-r','TOMATO.TEST','-P','isolated-fixture-only'])
        for principal in ('fixture-user','hive/localhost'):
            run([tools['kadmin.local'],'-r','TOMATO.TEST','-q',f'addprinc -randkey {principal}@TOMATO.TEST'])
            keytab=root/('hive.keytab' if principal.startswith('hive/') else 'user.keytab')
            run([tools['kadmin.local'],'-r','TOMATO.TEST','-q',f'ktadd -k {keytab} {principal}@TOMATO.TEST'])
        jaas=root/'jaas.conf'
        jaas.write_text(f'''TomatoFixture {{ com.sun.security.auth.module.Krb5LoginModule required
 useKeyTab=true keyTab="{root}/hive.keytab" principal="hive/localhost@TOMATO.TEST"
 storeKey=true doNotPrompt=true isInitiator=false; }};
''')
        env.update(TOMATO_HIVE_TEST_KERBEROS='isolated-loopback-realm',TOMATO_HIVE_JAVA_JAAS=str(jaas))
        with (root/'daemon.log').open('wb') as log:
            daemon=subprocess.Popen([tools['krb5kdc'],'-n','-r','TOMATO.TEST','-P',str(root/'pid')],env=env,stdout=log,stderr=subprocess.STDOUT)
            try:
                deadline=time.monotonic()+10
                while True:
                    if daemon.poll() is not None:raise RuntimeError('Owned KDC exited before readiness')
                    try:
                        with socket.create_connection(('127.0.0.1',port),timeout=.1):break
                    except OSError:
                        if time.monotonic()>deadline:raise RuntimeError('Owned KDC did not become ready')
                        time.sleep(.05)
                run([tools['kinit'],'-k','-t',str(root/'user.keytab'),'fixture-user@TOMATO.TEST'])
                yield env
            finally:
                if daemon.poll() is None:daemon.terminate()
                try:daemon.wait(timeout=5)
                except subprocess.TimeoutExpired:daemon.kill();daemon.wait(timeout=5)
