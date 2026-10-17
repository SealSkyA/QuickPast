# QuickPaste S2 自测：LocalSend v2 兼容 mock 设备（TCP 0.0.0.0:53318）
# 端点：register / info / prepare-upload / upload（upload 收到的字节写 %TEMP%\qp-mock-rx\{fileId}.bin）
# 用法：python tools\s2mock.py
import json, os, socket, sys, threading

PORT = 53318
RX = os.path.join(os.environ.get('TEMP', '.'), 'qp-mock-rx')
os.makedirs(RX, exist_ok=True)
srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(('0.0.0.0', PORT))
srv.listen(8)
print(f'mock 监听 0.0.0.0:{PORT}', flush=True)

def read_headers(conn):
    data = b''
    while b'\r\n\r\n' not in data:
        chunk = conn.recv(4096)
        if not chunk:
            return None
        data += chunk
    head, _, rest = data.partition(b'\r\n\r\n')
    return head, rest

def handle(conn):
    try:
        r = read_headers(conn)
        if r is None:
            conn.close(); return
        head, rest = r
        head_s = head.decode('latin1')
        lines = head_s.split('\r\n')
        method, url = lines[0].split(' ')[0], lines[0].split(' ')[1]
        path = url.split('?')[0]
        cl = 0
        for l in lines:
            if l.lower().startswith('content-length:'):
                cl = int(l.split(':')[1].strip())
        body = rest
        while len(body) < cl:
            chunk = conn.recv(cl - len(body))
            if not chunk:
                break
            body += chunk
        print(f'==> {method} {url} ({len(body)}B)', flush=True)
        if path == '/api/localsend/v2/register':
            resp = json.dumps({'alias': 'MockPC', 'version': '2.2', 'deviceModel': 'Windows',
                               'deviceType': 'desktop', 'fingerprint': 'MockFP-001', 'download': True}).encode()
        elif path == '/api/localsend/v2/info':
            resp = json.dumps({'alias': 'MockPC', 'version': '2.2', 'deviceModel': 'Windows',
                               'deviceType': 'desktop', 'fingerprint': 'MockFP-001', 'download': True}).encode()
        elif path == '/api/localsend/v2/prepare-upload':
            v = json.loads(body)
            files = {fid: f'tok-{fid}' for fid in v.get('files', {})}
            resp = json.dumps({'sessionId': 'mock-sess-1', 'files': files}).encode()
        elif path == '/api/localsend/v2/upload':
            file_id = 'unknown'
            for q in url.split('?')[1].split('&'):
                if q.startswith('fileId='):
                    file_id = q.split('=', 1)[1]
            dest = os.path.join(RX, f'{file_id}.bin')
            os.makedirs(RX, exist_ok=True)
            with open(dest, 'wb') as f:
                f.write(body)
            print(f'    upload 落盘 {dest} ({len(body)}B)', flush=True)
            resp = b''
        else:
            resp = b'{"error":"not found"}'
        hdr = (f'HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n'
               f'Content-Length: {len(resp)}\r\nConnection: close\r\n\r\n').encode('latin1')
        conn.sendall(hdr + resp)
        print('    RESP 已写', flush=True)
    except Exception as e:
        print(f'mock 异常: {e}', flush=True)
    finally:
        try:
            conn.close()
        except Exception:
            pass

while True:
    conn, addr = srv.accept()
    threading.Thread(target=handle, args=(conn,), daemon=True).start()
