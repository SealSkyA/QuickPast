# QuickPaste CDP 调试客户端：WebSocket → Runtime.evaluate
# 用法：python tools\cdp.py "<JS 表达式>"
import base64, json, os, socket, struct, sys, urllib.request

def get_ws_url():
    data = json.load(urllib.request.urlopen('http://127.0.0.1:9222/json', timeout=5))
    page = next((t for t in data if t.get('type') == 'page'), data[0])
    return page['webSocketDebuggerUrl']

def ws_connect(url):
    rest = url.split('://')[1]
    host, path = rest.split('/', 1)
    path = '/' + path
    hostname, port = host.split(':')
    s = socket.create_connection((hostname, int(port)), timeout=10)
    key = base64.b64encode(os.urandom(16)).decode()
    req = (f"GET {path} HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
           f"Sec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n")
    s.sendall(req.encode())
    resp = s.recv(4096)
    if b'101' not in resp.split(b'\r\n')[0]:
        raise Exception('握手失败: ' + resp[:200].decode('latin1'))
    return s

def ws_send(s, text):
    data = text.encode()
    mask = os.urandom(4)
    header = bytearray([0x81])
    ln = len(data)
    if ln < 126: header.append(0x80 | ln)
    elif ln < 65536: header.append(0x80 | 126); header += struct.pack('>H', ln)
    else: header.append(0x80 | 127); header += struct.pack('>Q', ln)
    masked = bytes(b ^ mask[i % 4] for i, b in enumerate(data))
    s.sendall(bytes(header) + mask + masked)

def ws_recv(s):
    hdr = s.recv(2)
    ln = hdr[1] & 0x7F
    if ln == 126: ln = struct.unpack('>H', s.recv(2))[0]
    elif ln == 127: ln = struct.unpack('>Q', s.recv(8))[0]
    return s.recv(ln)

def evaluate(expr):
    s = ws_connect(get_ws_url())
    ws_send(s, json.dumps({'id': 1, 'method': 'Runtime.evaluate',
                           'params': {'expression': expr, 'returnByValue': True, 'awaitPromise': True}}))
    out = ws_recv(s)
    s.close()
    return out.decode('utf-8', 'replace')

if __name__ == '__main__':
    expr = sys.argv[1] if len(sys.argv) > 1 else '1+1'
    print(evaluate(expr))
