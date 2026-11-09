#!/usr/bin/env python3
"""An HTTP CONNECT proxy that takes names alone, for the live tests.

  python3 -I scripts/names-proxy.py [--listen HOST:PORT]

The policy is the one of a proxy that allows listed names:

  CONNECT to a name on port 443, 80 or 8443   -> 200 Connection Established
  CONNECT to an address literal               -> 403 not on the allowlist
  any other port                              -> 403 not on the egress allowlist
  a name that resolves to a non-public address -> 403 not a public host
  any other method                            -> 405

The proxy resolves each name itself. It prints one line for each request:
the status, then the request line. Origin: the proxy of the test box of
podssh (scripts/box/proxy.py, 0BSD), reduced to the mode `--take names`.
"""

import argparse
import asyncio
import ipaddress
import socket
import sys

ALLOWED_PORTS = {443, 80, 8443}


def is_literal(host):
    try:
        ipaddress.ip_address(host.strip("[]"))
        return True
    except ValueError:
        return False


def log(line):
    print(line, flush=True)


async def pipe(reader, writer):
    try:
        while data := await reader.read(65536):
            writer.write(data)
            await writer.drain()
    except OSError:
        pass
    finally:
        try:
            writer.close()
        except OSError:
            pass


async def answer(writer, status, reason, request_line):
    log(f"{status} {request_line}")
    writer.write(f"HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\n\r\n".encode())
    await writer.drain()
    writer.close()


async def handle(reader, writer):
    try:
        head = await asyncio.wait_for(reader.readuntil(b"\r\n\r\n"), 30)
    except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, asyncio.TimeoutError, OSError):
        writer.close()
        return
    request_line = head.split(b"\r\n", 1)[0].decode("latin-1")
    parts = request_line.split()
    if len(parts) != 3 or parts[0] != "CONNECT":
        await answer(writer, 405, "Method Not Allowed", request_line)
        return
    host, _, port_text = parts[1].rpartition(":")
    host = host.strip("[]")
    if not port_text.isdigit() or int(port_text) not in ALLOWED_PORTS:
        await answer(writer, 403, "not on the egress allowlist", request_line)
        return
    if is_literal(host):
        await answer(writer, 403, "not on the allowlist", request_line)
        return
    port = int(port_text)
    try:
        infos = await asyncio.get_running_loop().getaddrinfo(host, port, type=socket.SOCK_STREAM)
    except OSError:
        await answer(writer, 502, "Bad Gateway", request_line)
        return
    addresses = [info[4][0] for info in infos]
    if not addresses or not all(ipaddress.ip_address(a.split("%")[0]).is_global for a in addresses):
        await answer(writer, 403, "not a public host", request_line)
        return
    try:
        up_reader, up_writer = await asyncio.wait_for(asyncio.open_connection(addresses[0], port), 15)
    except (OSError, asyncio.TimeoutError):
        await answer(writer, 502, "Bad Gateway", request_line)
        return
    log(f"200 {request_line}")
    writer.write(b"HTTP/1.1 200 Connection Established\r\n\r\n")
    await writer.drain()
    await asyncio.gather(pipe(reader, up_writer), pipe(up_reader, writer))


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen", default="127.0.0.1:3129")
    args = parser.parse_args()
    host, port = args.listen.rsplit(":", 1)
    server = await asyncio.start_server(handle, host, int(port))
    log(f"listening on {args.listen}")
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
