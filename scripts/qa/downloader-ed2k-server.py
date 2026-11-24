"""Independent, loopback-only eD2k server/peer for native Add-dialog acceptance.

Payload and MD4 are the RFC 1320 'abc' vector. No remote endpoint is contacted.
"""
import argparse, asyncio, json, struct
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument('--port-file', type=Path, required=True)
p.add_argument('--proof', type=Path, required=True)
args = p.parse_args()
digest = bytes.fromhex('A448017AAF21D8525FC10AE87AA6729D')
payload = b'abc'
proof = dict(loopbackOnly=True, sourceQueries=0, peerHellos=0, bytesSent=0,
             active=0, errors=[], expectedMd4=digest.hex(), expectedBytes=3)

def save():
    args.proof.write_text(json.dumps(proof, indent=2), encoding='utf-8')

async def read(reader):
    header = await reader.readexactly(6)
    length = struct.unpack_from('<I', header, 1)[0]
    if not 1 <= length <= 2_000_000:
        raise ValueError('Invalid frame length')
    return header[0], header[5], await reader.readexactly(length - 1)

async def send(writer, proto, opcode, body):
    writer.write(bytes([proto]) + struct.pack('<I', len(body)+1) + bytes([opcode]) + body)
    await writer.drain()

async def peer(reader, writer):
    proof['active'] += 1
    save()
    try:
        proto, op, hello = await read(reader)
        assert proto == 0xe3 and op == 1 and hello[0] == 16
        proof['peerHellos'] += 1
        await send(writer, 0xe3, 0x4c, hello[1:])
        while True:
            proto, op, body = await read(reader)
            if op == 0x4f:
                assert body == digest
                await send(writer, 0xe3, 0x50, digest + struct.pack('<H', 0))
            elif op == 0x51:
                await send(writer, 0xe3, 0x52, digest + struct.pack('<H', 0))
            elif op == 0x54:
                await send(writer, 0xc5, 0x60, struct.pack('<H', 1) + bytes(10))
                await send(writer, 0xe3, 0x55, b'')
            elif proto == 0xe3 and op == 0x47:
                assert len(body) == 40 and body[:16] == digest
                offsets = struct.unpack_from('<6I', body, 16)
                for start, end in zip(offsets[:3], offsets[3:]):
                    if start == end:
                        continue
                    assert 0 <= start < end <= len(payload)
                    await send(writer, 0xe3, 0x46, digest + struct.pack('<II', start, end) + payload[start:end])
                    proof['bytesSent'] += end - start
                    save()
    except (asyncio.IncompleteReadError, ConnectionError):
        pass
    except Exception as e:
        proof['errors'].append(type(e).__name__ + ': ' + str(e))
    finally:
        writer.close()
        try:
            await writer.wait_closed()
        except ConnectionError:
            pass
        proof['active'] -= 1
        save()

async def main():
    peer_server = await asyncio.start_server(peer, '127.0.0.1', 0)
    peer_port = peer_server.sockets[0].getsockname()[1]
    async def server(reader, writer):
        proof['active'] += 1
        save()
        try:
            proto, op, hello = await read(reader)
            assert proto == 0xe3 and op == 1 and len(hello) >= 26
            await send(writer, 0xe3, 0x40, bytes([127, 0, 0, 1]))
            while True:
                _, op, body = await read(reader)
                if op == 0x19:
                    assert len(body) == 20 and body[:16] == digest
                    assert struct.unpack_from('<I', body, 16)[0] == 3
                    proof['sourceQueries'] += 1
                    await send(writer, 0xe3, 0x42, digest + b'\x01\x7f\x00\x00\x01' + struct.pack('<H', peer_port))
                    save()
        except (asyncio.IncompleteReadError, ConnectionError):
            pass
        except Exception as e:
            proof['errors'].append(type(e).__name__ + ': ' + str(e))
        finally:
            writer.close()
            try:
                await writer.wait_closed()
            except ConnectionError:
                pass
            proof['active'] -= 1
            save()
    server = await asyncio.start_server(server, '127.0.0.1', 0)
    args.port_file.write_text(str(server.sockets[0].getsockname()[1]), encoding='ascii')
    save()
    async with server, peer_server:
        await asyncio.gather(server.serve_forever(), peer_server.serve_forever())

asyncio.run(main())
