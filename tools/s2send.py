# QuickPaste S2 自测：LocalSend v2 兼容发送端（发给 QuickPaste 接收实例）
# 用法：python tools\s2send.py [目标IP] [目标端口] [源目录]
# 流程：prepare-upload 声明 → 取 token → 逐文件裸字节 upload
import hashlib, json, os, sys, urllib.request, urllib.parse

IP = sys.argv[1] if len(sys.argv) > 1 else '127.0.0.1'
PORT = sys.argv[2] if len(sys.argv) > 2 else '53317'
SRC = sys.argv[3] if len(sys.argv) > 3 else os.path.join(os.environ.get('TEMP', '.'), 'qp-s2-src')
BASE = f'http://{IP}:{PORT}'

def post(path, body=None, raw=None):
    url = BASE + path
    if raw is not None:
        req = urllib.request.Request(url, data=raw, method='POST')
    else:
        req = urllib.request.Request(url, data=json.dumps(body).encode(), method='POST',
                                     headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(req, timeout=75) as r:
        return r.status, r.read()

files = {}
for name in sorted(os.listdir(SRC)):
    p = os.path.join(SRC, name)
    if os.path.isfile(p):
        data = open(p, 'rb').read()
        fid = name  # fileId 原样（真实客户端 UUID 风格）；upload query 时再 URL 编码
        files[fid] = {
            'id': fid,
            'fileName': name,
            'size': len(data),
            'sha256': hashlib.sha256(data).hexdigest(),
            'fileType': 'text/plain' if name.endswith('.txt') else 'application/octet-stream',
        }

info = {'alias': 'MockPC', 'version': '2.2', 'deviceModel': 'Windows',
        'deviceType': 'desktop', 'fingerprint': 'MockFP-001', 'download': True}
sess_id = 'mock-sess-2'
st, body = post('/api/localsend/v2/prepare-upload', {
    'info': info,
    'files': files,
    'sessionId': sess_id,
    'metadata': {},
})
print(f'prepare-upload -> {st} {body[:200]!r}')
if st != 200:
    sys.exit(1)
v = json.loads(body)
sess_id = v['sessionId']
for fid, meta in files.items():
    token = v['files'][fid]
    raw = open(os.path.join(SRC, meta['fileName']), 'rb').read()
    # upload query 中 fileId 需 URL 编码（中文/特殊字符）
    q = f'sessionId={sess_id}&fileId={urllib.parse.quote(fid)}&token={token}'
    st, body = post(f'/api/localsend/v2/upload?{q}', raw=raw)
    print(f'upload {meta["fileName"]} ({len(raw)}B) -> {st}')
    if st != 200:
        print('   响应:', body)
        sys.exit(2)
print('全部上传完成')
