# -*- coding: utf-8 -*-
"""
Token 用量采集 + 实时仪表盘服务（纯 Python 标准库，无第三方依赖）

每次请求页面时自动重新采集 OhMyAgent 会话数据，因此浏览器刷新即可看到最新用量。

用法：
    python serve.py                # 启动，默认端口 8756，自动打开浏览器
    python serve.py 9000           # 指定端口
    环境变量 OHMYAGENT_DATA_DIR 可覆盖数据目录

停止：关闭运行服务的终端窗口（Ctrl+C 亦可）。
"""

import datetime
import json
import os
import socket
import sys
import webbrowser
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

BASE_DIR = os.path.dirname(os.path.abspath(__file__))
TEMPLATE_PATH = os.path.join(BASE_DIR, 'dashboard.template.html')
OUT_DIR = os.path.join(BASE_DIR, 'data')

# 参数解析：位置参数为端口数字；--no-browser 为静默模式（开机自启用）
def parse_args():
    port = int(os.environ.get('PORT', '8756'))
    no_browser = bool(os.environ.get('NO_BROWSER'))
    for a in sys.argv[1:]:
        if a == '--no-browser':
            no_browser = True
        elif a.isdigit():
            port = int(a)
    return port, no_browser

PORT, NO_BROWSER = parse_args()


def port_alive(port):
    try:
        with socket.create_connection(('127.0.0.1', port), timeout=1):
            return True
    except OSError:
        return False


def get_data_dir():
    d = os.environ.get('OHMYAGENT_DATA_DIR')
    if d:
        return d
    return os.path.join(os.environ.get('APPDATA', ''), 'com.chaitin.baizhi.monkeycode', 'ohmyagent')


DATA_DIR = get_data_dir()


# ---------------------------------------------------------------------------
# JSONL 读取（容错：可处理字符串内换行、个别损坏记录）
# ---------------------------------------------------------------------------
def read_jsonl_records(path):
    recs = []
    try:
        with open(path, 'r', encoding='utf-8') as f:
            raw = f.read()
    except (OSError, UnicodeDecodeError):
        return recs
    dec = json.JSONDecoder()
    i, n = 0, len(raw)
    while i < n:
        while i < n and raw[i] in ' \t\r\n':
            i += 1
        if i >= n:
            break
        try:
            obj, end = dec.raw_decode(raw, i)
            recs.append(obj)
            i = end
        except json.JSONDecodeError:
            j = raw.find('\n', i)
            if j < 0:
                break
            i = j + 1
    return recs


# ---------------------------------------------------------------------------
# token 估算：中文/全角约 1.5 字符/token，其余约 4 字符/token
# ---------------------------------------------------------------------------
def estimate_tokens(text):
    if not text:
        return 0
    cjk = other = 0
    for ch in text:
        cp = ord(ch)
        if (0x3400 <= cp <= 0x4DBF or 0x4E00 <= cp <= 0x9FFF or
                0x3000 <= cp <= 0x30FF or 0xFF00 <= cp <= 0xFFEF):
            cjk += 1
        else:
            other += 1
    return int(math_ceil(cjk / 1.5) + math_ceil(other / 4.0))


def math_ceil(x):
    import math
    return math.ceil(x)


def assistant_text(msg):
    parts = []
    for c in msg.get('content_v2') or []:
        t = c.get('type')
        if t in ('text', 'thinking') and c.get('text'):
            parts.append(str(c['text']))
        elif t == 'tool_use' and c.get('input') is not None:
            parts.append(json.dumps(c['input'], ensure_ascii=False, separators=(',', ':')))
    return '\n'.join(parts)


def assistant_tool_calls(msg):
    return sum(1 for c in msg.get('content_v2') or [] if c.get('type') == 'tool_use')


def ssum(rows, key):
    return sum(int(r.get(key) or 0) for r in rows)


# ---------------------------------------------------------------------------
# 核心采集
# ---------------------------------------------------------------------------
def collect_all():
    sessions_dir = os.path.join(DATA_DIR, 'sessions')
    detail = []
    session_infos = []
    if not os.path.isdir(sessions_dir):
        raise RuntimeError('找不到会话目录：%s' % sessions_dir)

    for sid in sorted(os.listdir(sessions_dir)):
        mf = os.path.join(sessions_dir, sid, 'messages.jsonl')
        if not os.path.isfile(mf):
            continue
        recs = read_jsonl_records(mf)
        if not recs:
            continue

        metas = [r for r in recs if r.get('type') == 'meta']
        titles = [r for r in recs if r.get('type') == 'title']
        session_model = str(metas[0].get('model', '')) if metas else ''
        session_title = str(titles[-1].get('title', '')) if titles else ''
        if not session_title:
            session_title = '(未命名会话)'

        ctxs = sorted((r for r in recs if r.get('type') == 'context_tokens'), key=lambda r: r['ts'])
        assts = sorted((r for r in recs if r.get('type') == 'message' and r.get('role') == 'assistant'),
                       key=lambda r: r['ts'])
        if not ctxs:
            continue

        # 每条 assistant 消息归属到最近的前一条请求（ctx 时间 <= 消息时间）
        owner, ci = [], 0
        for m in assts:
            while ci + 1 < len(ctxs) and ctxs[ci + 1]['ts'] <= m['ts']:
                ci += 1
            owner.append(ci if ctxs[ci]['ts'] <= m['ts'] else -1)
        groups = [[] for _ in ctxs]
        for i, m in enumerate(assts):
            if owner[i] >= 0:
                groups[owner[i]].append(m)

        for k, ctx in enumerate(ctxs):
            ct = ctx.get('context_tokens') or {}
            dt = datetime.datetime.fromtimestamp(int(ctx['ts']))
            mdl = str(ct.get('model_id') or session_model or '(未知)')

            out_parts = [assistant_text(m) for m in groups[k]]
            out_text = '\n'.join(out_parts)
            out_chars = len(out_text)
            tool_calls = sum(assistant_tool_calls(m) for m in groups[k])

            detail.append({
                'ts': dt.strftime('%Y-%m-%d %H:%M:%S'),
                'date': dt.strftime('%Y-%m-%d'),
                'time': dt.strftime('%H:%M:%S'),
                'session_id': sid,
                'session_title': session_title,
                'model': mdl,
                'input_actual': int(ct.get('baseline_actual_tokens') or 0),
                'input_estimated': int(ct.get('baseline_estimated_tokens') or 0),
                'output_estimated': estimate_tokens(out_text),
                'output_chars': out_chars,
                'tool_calls': tool_calls,
                'message_count': int(ct.get('message_count') or 0),
                'fingerprint': str(ct.get('request_fingerprint') or ''),
            })

        sess_rows = [r for r in detail if r['session_id'] == sid]
        if sess_rows:
            session_infos.append({
                'id': sid,
                'title': session_title,
                'model': session_model,
                'requests': len(sess_rows),
                'input_actual': ssum(sess_rows, 'input_actual'),
                'input_estimated': ssum(sess_rows, 'input_estimated'),
                'output_estimated': ssum(sess_rows, 'output_estimated'),
                'total_estimated': ssum(sess_rows, 'input_actual') + ssum(sess_rows, 'output_estimated'),
                'first_ts': sess_rows[0]['ts'],
                'last_ts': sess_rows[-1]['ts'],
            })

    if not detail:
        raise RuntimeError('未找到任何包含 context_tokens 记录的会话。')

    by_day_rows, by_day_map = [], {}
    for r in detail:
        by_day_map.setdefault(r['date'], []).append(r)
    for date, rows in sorted(by_day_map.items()):
        by_day_rows.append({
            'date': date,
            'requests': len(rows),
            'input_actual': ssum(rows, 'input_actual'),
            'input_estimated': ssum(rows, 'input_estimated'),
            'output_estimated': ssum(rows, 'output_estimated'),
            'total_estimated': ssum(rows, 'input_actual') + ssum(rows, 'output_estimated'),
        })

    by_model_map = {}
    for r in detail:
        by_model_map.setdefault(r['model'], []).append(r)
    by_model_rows = [{
        'model': m,
        'requests': len(rows),
        'input_actual': ssum(rows, 'input_actual'),
        'output_estimated': ssum(rows, 'output_estimated'),
        'total_estimated': ssum(rows, 'input_actual') + ssum(rows, 'output_estimated'),
    } for m, rows in sorted(by_model_map.items())]

    by_session_rows = sorted(session_infos, key=lambda s: s['last_ts'], reverse=True)

    return {
        'generated_at': datetime.datetime.now().strftime('%Y-%m-%d %H:%M:%S'),
        'data_dir': DATA_DIR,
        'summary': {
            'requests': len(detail),
            'input_actual': ssum(detail, 'input_actual'),
            'input_estimated': ssum(detail, 'input_estimated'),
            'output_estimated': ssum(detail, 'output_estimated'),
            'output_chars': ssum(detail, 'output_chars'),
            'total_estimated': ssum(detail, 'input_actual') + ssum(detail, 'output_estimated'),
            'tool_calls': ssum(detail, 'tool_calls'),
            'sessions': len(session_infos),
        },
        'by_day': by_day_rows,
        'by_model': by_model_rows,
        'by_session': by_session_rows,
        'detail': detail,
    }


# ---------------------------------------------------------------------------
# 输出：usage_stats.json + usage_detail.csv（每次采集同步刷新）
# ---------------------------------------------------------------------------
def write_outputs(stats):
    os.makedirs(OUT_DIR, exist_ok=True)
    with open(os.path.join(OUT_DIR, 'usage_stats.json'), 'w', encoding='utf-8') as f:
        json.dump(stats, f, ensure_ascii=False, indent=2)

    def cell(v):
        s = str(v)
        return '"%s"' % s.replace('"', '""') if any(ch in s for ch in ',"\r\n') else s

    lines = ['时间,会话ID,会话标题,模型,输入token(实际),输入token(估算),输出token(估算),输出字符数,工具调用次数,上下文消息数']
    for r in stats['detail']:
        lines.append(','.join(cell(r[k]) for k in (
            'ts', 'session_id', 'session_title', 'model', 'input_actual',
            'input_estimated', 'output_estimated', 'output_chars', 'tool_calls', 'message_count')))
    with open(os.path.join(OUT_DIR, 'usage_detail.csv'), 'w', encoding='utf-8-sig', newline='') as f:
        f.write('\r\n'.join(lines) + '\r\n')


# ---------------------------------------------------------------------------
# 仪表盘 HTML 渲染
# ---------------------------------------------------------------------------
def render_dashboard(stats):
    with open(TEMPLATE_PATH, 'r', encoding='utf-8') as f:
        tmpl = f.read()
    json_str = json.dumps(stats, ensure_ascii=False, separators=(',', ':'))
    json_str = json_str.replace('<', '\\u003c')  # 防止 </script> 破坏内嵌块
    html = tmpl.replace('__STATS_JSON__', json_str).replace('__GENERATED_AT__', stats['generated_at'])
    return html


# ---------------------------------------------------------------------------
# HTTP 服务
# ---------------------------------------------------------------------------
class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        try:
            if self.path in ('/', '/index.html'):
                stats = collect_all()
                write_outputs(stats)
                body = render_dashboard(stats).encode('utf-8')
                ctype = 'text/html; charset=utf-8'
            elif self.path == '/api/stats':
                stats = collect_all()
                body = json.dumps(stats, ensure_ascii=False).encode('utf-8')
                ctype = 'application/json; charset=utf-8'
            else:
                body = b'404'
                ctype = 'text/plain'
                self.send_response(404)
                self.send_header('Content-Type', ctype)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            self.send_response(200)
            self.send_header('Content-Type', ctype)
            self.send_header('Cache-Control', 'no-store')
            self.send_header('Content-Length', str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception as e:  # noqa: BLE001
            msg = ('采集失败：%s' % e).encode('utf-8')
            self.send_response(500)
            self.send_header('Content-Type', 'text/plain; charset=utf-8')
            self.send_header('Content-Length', str(len(msg)))
            self.end_headers()
            self.wfile.write(msg)

    def log_message(self, *args):  # 静默访问日志
        pass


def main():
    if not os.path.isfile(TEMPLATE_PATH):
        print('缺少模板文件：%s' % TEMPLATE_PATH)
        sys.exit(1)

    url = 'http://127.0.0.1:%d/' % PORT
    try:
        server = ThreadingHTTPServer(('127.0.0.1', PORT), Handler)
    except OSError as e:
        if port_alive(PORT):
            print('检测到服务已在运行（%s），无需重复启动。' % url)
            if not NO_BROWSER:
                webbrowser.open(url)
            sys.exit(0)
        print('启动失败（端口 %d 无法绑定）：%s' % (PORT, e))
        print('可换端口：python serve.py 9000')
        sys.exit(1)

    print('=' * 56)
    print('  Token 用量实时仪表盘已启动：%s' % url)
    print('  每次刷新页面自动重新采集最新数据。')
    print('  停止服务：关闭本窗口、按 Ctrl+C，或运行 停止服务.bat。')
    print('=' * 56)
    if not NO_BROWSER:
        webbrowser.open(url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        print('\n已停止。')


if __name__ == '__main__':
    main()
