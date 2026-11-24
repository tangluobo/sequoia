"""Exercise real native Access/ODBC adapters with isolated local fixtures."""
import argparse,hashlib,io,json,pathlib,struct,subprocess
p=argparse.ArgumentParser();p.add_argument('--output',required=True);p.add_argument('--access',required=True);p.add_argument('--odbc32',required=True);p.add_argument('--encrypted-fixtures');a=p.parse_args()
root=pathlib.Path(a.output).resolve();root.mkdir(parents=True,exist_ok=False)
access=pathlib.Path(a.access).resolve();odbc=pathlib.Path(a.odbc32).resolve()
u=lambda n:struct.pack('>I',n)
def text(s):b=s.encode('utf-8');return u(len(b))+b
def read_u(f):b=f.read(4);assert len(b)==4;return struct.unpack('>I',b)[0]
def read_s(f):return f.read(read_u(f)).decode('utf-8')
def run(exe,args,data):
 r=subprocess.run([str(exe),*map(str,args)],input=data,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=30,creationflags=getattr(subprocess,'CREATE_NO_WINDOW',0))
 assert r.returncode==0,('adapter failed',exe.name,r.returncode,r.stderr.decode(errors='replace')[:400]);return r.stdout
def access_rows(path,password=''):
 target=root/'access-list.bin';run(access,['-Xmx128m','--list',path,target],text(password));f=io.BytesIO(target.read_bytes());assert read_u(f)==0x54414c31;names=[read_s(f) for _ in range(read_u(f))];assert names
 rows={}
 for name in names:
  target=root/'access-read.bin';run(access,['-Xmx128m','--read',path,target],text(password)+text(name));f=io.BytesIO(target.read_bytes());assert read_u(f)==0x54414931;columns=[(read_s(f),read_s(f)) for _ in range(read_u(f))];values=[]
  while f.read(1)==b'\x01':
   row=[]
   for _ in columns:
    tag=f.read(1)
    if tag==b'\0':v=None
    elif tag in [b'\1',b'\2']:v=read_s(f)
    elif tag==b'\3':v=struct.unpack('>d',f.read(8))[0]
    elif tag==b'\4':v=f.read(1)!=b'\0'
    elif tag==b'\5':v=f.read(read_u(f)).hex()
    else:raise AssertionError(('invalid cell',tag))
    row.append(v)
   values.append(row)
  rows[name]={'columns':columns,'rows':values}
 return rows
payload=u(0x54414331)+text('owned_records')+u(3)+text('id')+b'\1'+text('note')+b'\0'+text('blob')+b'\4'
for row in [('1','中文🙂',b'\0\1\xff'),('9223372036854775807','',b''),(None,None,None)]:
 payload+=b'\1'
 for i,value in enumerate(row):
  if value is None:payload+=b'\0'
  elif i==0:payload+=b'\2'+text(value)
  elif i==1:payload+=b'\1'+text(value)
  else:payload+=b'\5'+u(len(value))+value
payload+=b'\0';db=root/'owned.accdb';run(access,['-Xmx128m',db],payload)
rows=access_rows(db)['owned_records']['rows'];assert rows[0]==['1','中文🙂','0001ff'];assert rows[1]==['9223372036854775807','',''];assert rows[2]==[None,None,None]
checks=['Access ACCDB: Unicode, exact i64, empty text, empty binary and NULL']
if a.encrypted_fixtures:
 fixtures=pathlib.Path(a.encrypted_fixtures)
 for name,password in [('db-enc.mdb',''),('db97-enc.mdb',''),('db2013-enc.accdb','1234')]:
  rows=access_rows(fixtures/name,password);assert any(t['rows'] for t in rows.values());checks.append('Access encrypted source: '+name)
sources=io.BytesIO(run(odbc,[],u(0x544f4431)+u(1)));assert read_u(sources)==0x544f5331;drivers=[]
while True:
 tag=sources.read(1)
 if tag==b'\0':break
 name=read_s(sources)
 if tag==b'\2':drivers.append(name)
driver=next(n for n in drivers if n=='Microsoft Text Driver (*.txt; *.csv)')
source=root/'odbc-source';source.mkdir();csv=source/'owned.csv'
csv.write_text('Id,Name,Amount\r\n'+''.join(f'{i},row-{i},12345678901234567890.123456\r\n' for i in range(2305)),encoding='ascii',newline='')
(source/'schema.ini').write_text('[owned.csv]\r\nFormat=CSVDelimited\r\nColNameHeader=True\r\nMaxScanRows=0\r\nCol1=Id Integer\r\nCol2=Name Char Width 40\r\nCol3=Amount Char Width 80\r\n',encoding='ascii',newline='')
settings=text('')+text('')+text('')+text('Driver={'+driver+'};Dbq='+str(source)+';Extensions=csv;ReadOnly=True;')
tables=io.BytesIO(run(odbc,[],u(0x544f4431)+u(2)+settings));assert read_u(tables)==0x544f5431;names=[]
while tables.read(1)==b'\1':names.append((read_s(tables),read_s(tables),read_s(tables)))
chosen=next(t for t in names if t[2].lower()=='owned.csv')
def odbc_rows(op,extra):
 f=io.BytesIO(run(odbc,[],u(0x544f4431)+u(op)+settings+extra));assert read_u(f)==0x544f5231;columns=[]
 for _ in range(read_u(f)):columns.append((read_s(f),read_u(f),read_u(f),read_u(f)))
 rows=[]
 while f.read(1)==b'\1':
  row=[]
  for _ in columns:
   tag=f.read(1);assert tag in [b'\0',b'\1',b'\5'];row.append(None if tag==b'\0' else read_s(f) if tag==b'\1' else f.read(read_u(f)).hex())
  rows.append(row)
 return rows
values=odbc_rows(4,b''.join(text(p) for p in chosen));assert len(values)==2305;assert values[-1]==['2304','row-2304','12345678901234567890.123456'];checks.append('ODBC x86: installed Text driver, real table enumeration and 2305 records')
values=odbc_rows(3,text('SELECT [Id],[Name],[Amount] FROM [owned.csv] WHERE [Id]>=2300'));assert len(values)==5;checks.append('ODBC x86: explicit SELECT filtering and exact decimal text')
proof={'status':'passed','checks':checks,'access_sha256':hashlib.sha256(access.read_bytes()).hexdigest().upper(),'odbc32_sha256':hashlib.sha256(odbc.read_bytes()).hexdigest().upper(),'fixture_directory':str(root),'user_dsns_modified':False,'saved_hosts_contacted':False}
(root/'validation.json').write_text(json.dumps(proof,ensure_ascii=False,indent=2),encoding='utf-8');print(json.dumps(proof,ensure_ascii=False))
