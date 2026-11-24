"""Own a loopback relay recording only negotiated RDP security scalar fields.

Weak-method QA restricts the initial client advertisement, so the independent
server actually selects 40/56 bits instead of its normal strongest method.
Encrypted traffic and credentials are never inspected, changed, or recorded.
"""
from contextlib import contextmanager
import socket, struct, threading

@contextmanager
def negotiation(backend, methods=None, expected=None):
    stop=threading.Event();lock=threading.Lock();sockets=[];threads=[];selected=[];errors=[]
    listener=socket.socket();listener.bind(('127.0.0.1',0));listener.listen(8);listener.settimeout(.2)
    port=listener.getsockname()[1]
    def close(sock):
        try:sock.shutdown(socket.SHUT_RDWR)
        except OSError:pass
        sock.close()
    def inspect(frame,client):
        if len(frame)<10 or frame[:2]!=b'\x03\0':return frame,False
        tag=b'\x7f\x65' if client else b'\x7f\x66'
        if frame[7:9]!=tag:return frame,False
        marker=b'\x02\xc0\x0c\0' if client else b'\x02\x0c'
        starts=[]
        for index in range(9,len(frame)-11):
            if frame[index:index+len(marker)]==marker:
                length=struct.unpack_from('<H',frame,index+2)[0]
                valid=length>=12 and index+length<=len(frame)
                if not client and valid:
                    method,level=struct.unpack_from('<II',frame,index+4)
                    valid=method in (0,1,2,8,16) and 0<=level<=4
                if valid:starts.append(index)
        if len(starts)!=1:raise ValueError('GCC security block was not unique')
        index=starts[0]
        if client:
            if methods is not None:
                frame=bytearray(frame);struct.pack_into('<I',frame,index+4,methods);frame=bytes(frame)
        else:
            method,level=struct.unpack_from('<II',frame,index+4)
            with lock:selected.append({'method':method,'level':level})
            if expected is not None and (method,level)!=expected:raise ValueError(f'Server selected {(method,level)}, expected {expected}')
        return frame,True
    def pump(source,target,client):
        pending=bytearray();done=False
        try:
            while not stop.is_set():
                try:chunk=source.recv(65536)
                except socket.timeout:continue
                if not chunk:break
                if done:target.sendall(chunk);continue
                pending.extend(chunk)
                while len(pending)>=4:
                    if pending[:2]!=b'\x03\0':raise ValueError('Unexpected pre-GCC packet')
                    length=struct.unpack_from('>H',pending,2)[0]
                    if length<7:raise ValueError('Malformed pre-GCC TPKT')
                    if len(pending)<length:break
                    frame=bytes(pending[:length]);del pending[:length]
                    frame,done=inspect(frame,client);target.sendall(frame)
                    if done:
                        if pending:target.sendall(pending);pending.clear()
                        break
                if len(pending)>65535:raise ValueError('GCC relay buffer exceeded limit')
        except OSError:
            pass
        except Exception as error:
            with lock:errors.append(str(error))
        finally:
            close(source);close(target)
    def accept():
        while not stop.is_set():
            try:client,_=listener.accept()
            except socket.timeout:continue
            except OSError:break
            try:server=socket.create_connection(('127.0.0.1',backend),timeout=2)
            except OSError:close(client);continue
            client.settimeout(1);server.settimeout(1)
            with lock:sockets.extend([client,server])
            for source,target,is_client in [(client,server,True),(server,client,False)]:
                thread=threading.Thread(target=pump,args=(source,target,is_client),daemon=True);thread.start();threads.append(thread)
    owner=threading.Thread(target=accept,daemon=True);owner.start()
    try:yield port,selected,errors
    finally:
        stop.set();listener.close();owner.join(timeout=3)
        for sock in sockets:close(sock)
        for thread in threads:thread.join(timeout=3)
