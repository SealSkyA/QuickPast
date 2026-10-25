// LocalSend v2 兼容网络层（对齐 PC 端 src-tauri/src/net.rs 行为）：
//  - UDP 组播发现 224.0.0.167:53318（announce → HTTP register 单播回执）
//  - HTTP 服务 0.0.0.0:53318（register/info/prepare-upload/upload/cancel）
//  - 发送客户端（prepare-upload → 逐文件裸字节流 upload；文本走临时 .txt 文件通道）
import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../models.dart';

/// 网络事件总线：UI 通过 ValueListenable 订阅；设置项变更可 bumpSettings 通知 UI 联动
class LocalSendService extends ChangeNotifier {
  LocalSendService._();
  static final LocalSendService instance = LocalSendService._();

  /// 本机设置变更通知（别名等 UI 展示的字段修改后调用）
  void bumpSettings() => notifyListeners();

  static int kPort = 53318; // 可覆盖（测试环境避免与本机电脑端冲突）
  static const String kMcAddr = '224.0.0.167';
  static const String kVersion = '2.2';
  static const int kDeviceTtlMs = 25000;
  static const Duration kConfirmTimeout = Duration(seconds: 60);
  static const Duration kAnnounceInterval = Duration(seconds: 10);

  // ── 本机身份（设置项，简单内存态 + 可被 UI 修改）──
  String alias = '我的手机';
  String fingerprint = '';
  bool online = true;
  String deviceModel = 'Android Phone';
  String saveDir = ''; // 空 = 默认外部目录/下载
  bool wifiOnly = true;
  bool vibrate = true;
  bool autoAccept = false;

  // ── 收藏的设备（按 fingerprint 持久化，重启保留）──
  final ValueNotifier<Set<String>> favs = ValueNotifier({});

  bool isFav(String fp) => favs.value.contains(fp);

  Future<void> loadFavs() async {
    try {
      final prefs = await SharedPreferences.getInstance();
      final list = prefs.getStringList('favFps') ?? const [];
      favs.value = list.toSet();
    } catch (_) {}
  }

  Future<void> toggleFav(String fp) async {
    final s = Set<String>.from(favs.value);
    if (!s.add(fp)) s.remove(fp);
    favs.value = s;
    try {
      final prefs = await SharedPreferences.getInstance();
      await prefs.setStringList('favFps', s.toList());
    } catch (_) {}
  }

  // ── 状态 ──
  final ValueNotifier<List<Device>> devices = ValueNotifier<List<Device>>([]);
  /// 待确认的接收请求队列（UI 顶部展示，逐个确认）
  final ValueNotifier<List<IncomingSession>> pending = ValueNotifier<List<IncomingSession>>([]);
  /// 进行中的接收会话（进度展示）+ 已完成（发 done 事件）
  final Map<String, IncomingSession> _sessions = {};
  final ValueNotifier<List<IncomingSession>> activeSessions = ValueNotifier([]);
  /// 接收完成事件（历史/Toast）
  final StreamController<Map<String, dynamic>> rcvDone = StreamController.broadcast();
  /// 发送完成/失败事件（Toast/历史）
  final StreamController<Map<String, dynamic>> sendDone = StreamController.broadcast();
  /// 发送进度 (index, total, file)
  final ValueNotifier<Map<String, dynamic>?> sendProgress = ValueNotifier(null);
  /// 发送取消请求（置位后上传流/循环中断，并通知对端清理半成品）
  bool _sendCancel = false;
  /// 活动发送的根地址记录（`base|sessionId`），取消时向对端发 HTTP cancel
  final List<String> _sendRoots = [];

  RawDatagramSocket? _udp;
  Timer? _announceTimer;
  final Map<String, Completer<bool>> _confirms = {}; // sessionId → 用户决定

  bool _started = false;

  Map<String, dynamic> get myInfo => {
        'alias': alias,
        'version': kVersion,
        'deviceModel': deviceModel,
        'deviceType': 'mobile',
        'fingerprint': fingerprint,
        'port': kPort,
        'protocol': 'http',
        'download': true,
      };

  Future<void> initFingerprint(Future<String> Function() persistGet) async {
    fingerprint = await persistGet().catchError((_) => _genFp());
    if (fingerprint.isEmpty) fingerprint = _genFp();
  }

  static String _genFp() {
    final r = Random.secure();
    final b = List<int>.generate(16, (_) => r.nextInt(256));
    return b.map((x) => x.toRadixString(16).padLeft(2, '0')).join();
  }

  /// 启动 UDP 组播发现 + HTTP 服务（幂等；各自独立 try，
  /// UDP 失败不阻断 HTTP——否则 _started 卡死导致接收服务永不启动）
  Future<void> start() async {
    if (_started) return;
    _started = true;
    try {
      await _startUdp();
    } catch (e) {
      debugPrint('LocalSend UDP start error: $e');
    }
    try {
      await _startHttp();
    } catch (e) {
      debugPrint('LocalSend HTTP start error: $e');
    }
  }

  Future<void> _startUdp() async {
    _announceTimer?.cancel();
    _announceTimer = null;
    final sock = await RawDatagramSocket.bind(InternetAddress.anyIPv4, kPort);
    try {
      sock.joinMulticast(InternetAddress(kMcAddr));
    } catch (e) {
      debugPrint('join multicast: $e（继续单播回执）');
    }
    sock.listen((event) {
      if (event != RawSocketEvent.read) return;
      final d = sock.receive();
      if (d == null) return;
      _handleAnnounce(utf8.decode(d.data, allowMalformed: true), d.address.address);
      _prune();
    });
    _udp = sock;
    // 初始三连发
    _sendAnnounce();
    _sendAnnounce();
    _sendAnnounce();
    _announceTimer = Timer.periodic(kAnnounceInterval, (_) {
      if (online) _sendAnnounce();
      _prune();
    });
  }

  void _sendAnnounce([InternetAddress? target, int? port]) {
    final s = _udp;
    if (s == null || !online) return;
    final msg = utf8.encode(jsonEncode({...myInfo, 'announce': true}));
    try {
      if (target != null && port != null) {
        s.send(msg, target, port); // 单播唤醒对端
      } else {
        s.send(msg, InternetAddress(kMcAddr), kPort);
      }
    } catch (_) {}
  }

  void _handleAnnounce(String text, String fromIp) {
    Map<String, dynamic> v;
    try {
      v = jsonDecode(text) as Map<String, dynamic>;
    } catch (_) {
      return;
    }
    final fp = (v['fingerprint'] as String?) ?? '';
    if (fp.isEmpty || fp == fingerprint) return;
    // 组播报文在 Wi-Fi 上偶发损坏 → 丢弃，等 HTTP register 正常覆盖
    final alias = (v['alias'] as String?) ?? '';
    if (alias.contains('\uFFFD')) return;
    _upsert(Device.fromAnnounce(v, fromIp));
    // 单播回执 register（在线时；离线则别人列表无我）
    if (online) {
      _post('http://$fromIp:${(v['port'] as num?)?.toInt() ?? kPort}/api/localsend/v2/register', myInfo).catchError((_) {});
    }
  }

  void _upsert(Device d) {
    final list = List<Device>.from(devices.value);
    final i = list.indexWhere((x) => x.fingerprint == d.fingerprint);
    if (i >= 0) {
      list[i] = d;
    } else {
      list.add(d);
    }
    devices.value = list;
  }

  void _prune() {
    final cut = DateTime.now().millisecondsSinceEpoch - kDeviceTtlMs;
    final before = devices.value.length;
    final list = devices.value.where((d) => d.lastSeen >= cut).toList();
    if (list.length != before) devices.value = list;
  }

  /// 手动刷新（发送页/我的页按钮 + 前台恢复）：重建发现通道（后台期间
  /// socket/组播成员关系可能失效）+ 广播自己 + 单播唤醒已知设备回执。
  Future<void> refresh() async {
    final old = _udp;
    _udp = null;
    try {
      old?.close();
    } catch (_) {}
    try {
      await _startUdp();
    } catch (e) {
      debugPrint('refresh rebind: $e');
      return;
    }
    // 组播收不到时，逐个单播已知设备（对端会回 register 恢复列表）
    for (final d in List.of(devices.value)) {
      _sendAnnounce(InternetAddress(d.ip), d.port);
    }
  }

  /// Android 组播锁：不获取 MulticastLock 时系统默认丢弃组播报文，
  /// 表现为「首次能发现、后台回来搜不到」。锁在后台可能被系统释放，
  /// 前台恢复时应重新获取。
  static const MethodChannel _wifiChannel = MethodChannel('qp_wifi');

  Future<void> acquireMulticast() async {
    try {
      await _wifiChannel.invokeMethod('acquireMulticast');
    } catch (_) {}
  }

  /* ────────────── HTTP 服务（接收端） ────────────── */

  Future<void> _startHttp() async {
    final server = await HttpServer.bind(InternetAddress.anyIPv4, kPort);
    server.listen((req) {
      _handleHttp(req);
    });
  }

  Future<void> _handleHttp(HttpRequest req) async {
    try {
      final path = req.uri.path;
      final method = req.method;
      if (method == 'GET' && path == '/api/localsend/v2/info') {
        _json(req, 200, myInfo);
        return;
      }
      if (method == 'GET' && path == '/') {
        req.response
          ..statusCode = 200
          ..headers.contentType = ContentType.html
          ..write('<!doctype html><meta charset=utf-8><title>QuickCopy</title>'
              '<body style="font-family:system-ui;display:flex;align-items:center;justify-content:center;height:90vh;background:#e9edf2">'
              '<div style="text-align:center"><h1>QuickCopy</h1>'
              '<p>手机端传输服务运行中（port $kPort）</p></div>')
          ..close();
        return;
      }
      if (method == 'POST' && path == '/api/localsend/v2/register') {
        final body = await _readBody(req);
        final v = _tryJson(utf8.decode(body));
        if (v != null) {
          final src = req.connectionInfo?.remoteAddress.address ?? '';
          _upsert(Device.fromAnnounce({...v, 'port': kPort}, src));
        }
        _json(req, 200, myInfo);
        return;
      }
      if (method == 'POST' && path == '/api/localsend/v2/prepare-upload') {
        await _handlePrepareUpload(req);
        return;
      }
      if (method == 'POST' && path == '/api/localsend/v2/upload') {
        await _handleUpload(req);
        return;
      }
      if (method == 'POST' && path == '/api/localsend/v2/cancel') {
        final sid = req.uri.queryParameters['sessionId'];
        if (sid != null) _rejectSession(sid);
        req.response..statusCode = 200..close();
        return;
      }
      _text(req, 404, 'not found');
    } catch (e) {
      debugPrint('http handler error: $e');
      try {
        req.response..statusCode = 500..close();
      } catch (_) {}
    }
  }

  Future<void> _handlePrepareUpload(HttpRequest req) async {
    final body = await _readBody(req);
    final v = _tryJson(utf8.decode(body));
    if (v == null || v['files'] is! Map || (v['files'] as Map).isEmpty) {
      _json(req, 400, {'error': 'bad request'});
      return;
    }
    final info = (v['info'] as Map<String, dynamic>?) ?? {};
    final senderIp = req.connectionInfo?.remoteAddress.address ?? '';
    final senderAlias = (info['alias'] as String?) ?? '?';
    final sessionId = _genUuid();
    final files = <IncomingFile>[];
    (v['files'] as Map).forEach((k, fv) {
      if (fv is Map) files.add(IncomingFile.fromJson(k.toString(), Map<String, dynamic>.from(fv)));
    });
    final sess = IncomingSession(
      sessionId: sessionId,
      senderIp: senderIp,
      senderAlias: senderAlias,
      files: files,
      state: 'Pending',
      createdAt: DateTime.now(),
    );
    _sessions[sessionId] = sess;
    final completer = Completer<bool>();
    _confirms[sessionId] = completer;

    // UI 弹确认
    pending.value = [...pending.value, sess];

    bool accept;
    if (autoAccept) {
      accept = true;
    } else {
      try {
        accept = await completer.future.timeout(kConfirmTimeout, onTimeout: () => false);
      } catch (_) {
        accept = false;
      }
    }
    _confirms.remove(sessionId);
    if (!accept) {
      sess.state = 'Rejected';
      // 超时/拒绝的会话必须从确认队列移除，否则失效请求卡会堆积在接收页
      pending.value = pending.value.where((s) => s.sessionId != sessionId).toList();
      _json(req, 403, {'message': 'rejected'});
      _syncActive();
      return;
    }
    sess.state = 'Accepted';
    // 自动保存路径没有经过 acceptSession，需在此移除确认队列中的卡
    pending.value = pending.value.where((s) => s.sessionId != sessionId).toList();
    final tokens = <String, String>{};
    for (final f in sess.files) {
      f.token ??= _genUuid();
      tokens[f.fileId] = f.token!;
    }
    _syncActive();
    _json(req, 200, {'sessionId': sessionId, 'files': tokens});
  }

  Future<void> _handleUpload(HttpRequest req) async {
    final q = req.uri.queryParameters;
    final sessionId = q['sessionId'];
    final fileId = q['fileId'];
    final token = q['token'];
    final ip = req.connectionInfo?.remoteAddress.address ?? '';
    if (sessionId == null || fileId == null || token == null) {
      _text(req, 422, 'missing params');
      return;
    }
    final sess = _sessions[sessionId];
    if (sess == null || sess.state == 'Rejected') {
      _text(req, 403, 'session not active');
      return;
    }
    // IP 绑定
    if (sess.senderIp != ip) {
      _text(req, 403, 'ip mismatch');
      return;
    }
    final f = sess.files.where((x) => x.fileId == fileId).firstOrNull;
    if (f == null) {
      _text(req, 422, 'unknown fileId');
      return;
    }
    if (f.token != token || f.state == 'Finished') {
      _text(req, 403, 'bad token');
      return;
    }

    f.state = 'InProgress';
    sess.state = 'InProgress';
    _syncActive();

    // 文本消息 → 剪贴板
    if (f.isText) {
      final content = await _readBody(req);
      final text = utf8.decode(content, allowMalformed: true);
      ClipboardDelegate.copy(text);
      f.state = 'Finished';
      _text(req, 200, '');
      _maybeDone(sess);
      return;
    }

    // 文件落盘（流式，带大小/可选 sha256 校验）
    final dir = await _resolveSaveDir();
    final targetPath = _uniquePath(dir, _safeName(f.fileName));
    try {
      await dir.create(recursive: true);
      final sink = File(targetPath).openWrite();
      var written = 0;
      final sha = Sha256Impl();
      // 进度同步节流：每 chunk 都 _syncActive() 会在收到大文件时以极高频率
      // 重建进度卡（30MB ≈ 数千次），把主 isolate 的渲染管线占满导致 socket
      // 接收饿死（大文件上传假死）。节流到每 32 chunk 或 ≥100ms 一次即可。
      var syncTick = 0;
      var lastSync = DateTime.now();
      await for (final chunk in req) {
        sink.add(chunk);
        sha.update(chunk);
        written += chunk.length;
        f.written = written;
        if (syncTick++ % 32 == 0 || DateTime.now().difference(lastSync).inMilliseconds >= 100) {
          _syncActive();
          lastSync = DateTime.now();
        }
      }
      await sink.flush();
      await sink.close();
      if (f.size > 0 && written != f.size) {
        f.state = 'Failed';
        _text(req, 422, 'size mismatch');
        _maybeDone(sess);
        return;
      }
      if (f.sha256 != null && f.sha256!.isNotEmpty) {
        final hex = sha.digestHex();
        if (hex.toLowerCase() != f.sha256!.toLowerCase()) {
          f.state = 'Failed';
          _text(req, 422, 'sha256 mismatch');
          _maybeDone(sess);
          return;
        }
      }
      f.state = 'Finished';
      f.path = targetPath;
      _text(req, 200, '');
      _maybeDone(sess);
    } catch (e) {
      f.state = 'Failed';
      _text(req, 500, 'disk error');
      _maybeDone(sess);
    }
  }

  void _maybeDone(IncomingSession sess) {
    _syncActive();
    if (!sess.allDone) return;
    sess.state = sess.files.every((f) => f.state == 'Finished') ? 'Finished' : 'Canceled';
    rcvDone.add({'sessionId': sess.sessionId, 'senderAlias': sess.senderAlias, 'files': sess.files});
  }

  Future<Directory> _resolveSaveDir() async {
    if (saveDir.isNotEmpty) {
      final d = Directory(saveDir);
      if (await d.exists()) return d;
    }
    // 默认：外部下载目录（用户可见）；无存储权限时降级到 App 专属目录保证落盘
    if (Platform.isAndroid) {
      final dl = Directory('/storage/emulated/0/Download/QuickCopy');
      try {
        await dl.create(recursive: true);
        final probe = File('${dl.path}/.qp_probe');
        await probe.writeAsString('ok');
        await probe.delete();
        return dl;
      } catch (_) {
        // 降级：App 专属外部目录（path_provider，无需存储权限，可靠落盘）
        try {
          final ext = await getExternalStorageDirectory();
          final dir = Directory('${ext?.path}/QuickCopy');
          await dir.create(recursive: true);
          return dir;
        } catch (_) {
          return Directory.systemTemp;
        }
      }
    }
    return Directory.systemTemp.createTemp('qp_rcv');
  }

  /// 已存在的目标 → 追加（N）
  String _uniquePath(Directory dir, String name) {
    var base = dir.path;
    var target = '$base/$name';
    var n = 1;
    while (File(target).existsSync()) {
      final dot = name.lastIndexOf('.');
      final stem = dot > 0 ? name.substring(0, dot) : name;
      final ext = dot > 0 ? name.substring(dot) : '';
      final cand = '$stem（$n）$ext';
      target = '$base/$cand';
      n++;
    }
    return target;
  }

  String _safeName(String name) {
    final sb = StringBuffer();
    name = name.replaceAll('\\', '_').replaceAll('/', '_');
    for (final c in name.split('')) {
      if (c == ':' || c == '*' || c == '?' || c == '"' || c == '<' || c == '>' || c == '|') {
        sb.write('_');
      } else {
        sb.write(c);
      }
    }
    final out = sb.toString();
    return out.isEmpty ? 'unnamed' : out;
  }

  /// 用户接受（可选子集 fileIds）
  void acceptSession(String sessionId, [List<String>? fileIds]) {
    final sess = _sessions[sessionId];
    if (sess != null && fileIds != null) {
      sess.files.removeWhere((f) => !fileIds.contains(f.fileId));
      for (final f in sess.files) {
        f.token ??= _genUuid();
      }
    }
    pending.value = pending.value.where((s) => s.sessionId != sessionId).toList();
    _confirms[sessionId]?.complete(true);
  }

  void rejectSession(String sessionId) {
    pending.value = pending.value.where((s) => s.sessionId != sessionId).toList();
    _confirms[sessionId]?.complete(false);
  }

  void _rejectSession(String sessionId) {
    final sess = _sessions[sessionId];
    if (sess != null) {
      sess.state = 'Canceled';
      _syncActive();
    }
    rejectSession(sessionId);
  }

  void _syncActive() {
    activeSessions.value = _sessions.values
        .where((s) => s.state == 'Accepted' || s.state == 'InProgress')
        .toList();
  }

  /* ────────────── 发送客户端 ────────────── */

  /// 发送一批条目（文件 item 走流式上传；文本 item 走临时 .txt）
  Future<void> sendItems(Device dev, List<SendItem> items) async {
    _sendCancel = false;
    _sendRoots.clear();
    sendProgress.value = {'index': 0, 'total': items.length, 'file': items.first.name};
    final filesMap = <String, Map<String, dynamic>>{};
    final ids = <String>[];
    for (final it in items) {
      final id = _genUuid();
      filesMap[id] = it.toPrepareJson(id);
      ids.add(id);
    }
    final base = 'http://${dev.ip}:${dev.port}/api/localsend/v2';
    // prepare-upload（对端确认最长 60s）
    late Map<String, dynamic> v;
    try {
      final resp = await _postWithTimeout(
        '$base/prepare-upload',
        jsonEncode({'info': myInfo, 'files': filesMap}),
        const Duration(seconds: 70),
      );
      v = jsonDecode(resp) as Map<String, dynamic>;
    } catch (e) {
      sendDone.add({'ok': false, 'device': dev.alias, 'error': _mapErr(e)});
      sendProgress.value = null;
      return;
    }
    final sessionId = (v['sessionId'] as String?) ?? '';
    final tokens = (v['files'] as Map<String, dynamic>?) ?? {};
    if (sessionId.isEmpty) {
      sendDone.add({'ok': false, 'device': dev.alias, 'error': '对端未返回会话'});
      sendProgress.value = null;
      return;
    }
    // 登记活动发送根：取消时向对端发 HTTP cancel 清理半成品
    _sendRoots.add('$base|$sessionId');
    // 逐文件上传
    var index = 0;
    for (final id in ids) {
      if (_sendCancel) {
        await _notifyPeerCancel();
        sendProgress.value = null;
        sendDone.add({'ok': false, 'device': dev.alias, 'error': '已取消', 'canceled': true});
        return;
      }
      index++;
      final token = tokens[id]?.toString() ?? '';
      if (token.isEmpty) {
        sendDone.add({'ok': false, 'device': dev.alias, 'error': '对端未接受部分内容'});
        sendProgress.value = null;
        return;
      }
      final it = items[ids.indexOf(id)];
      final url = '$base/upload?sessionId=$sessionId&fileId=$id&token=$token';
      try {
        if (it.isFileLike) {
          await _uploadFile(url, it);
        } else {
          if (_sendCancel) throw StateError('已取消');
          await _postWithTimeout(url, it.text ?? '', const Duration(seconds: 60));
        }
        sendProgress.value = {'index': index, 'total': ids.length, 'file': it.name};
      } catch (e) {
        if (_sendCancel) {
          await _notifyPeerCancel();
          sendDone.add({'ok': false, 'device': dev.alias, 'error': '已取消', 'canceled': true});
        } else {
          sendDone.add({'ok': false, 'device': dev.alias, 'error': _mapErr(e)});
        }
        sendProgress.value = null;
        return;
      }
    }
    sendProgress.value = null;
    sendDone.add({'ok': true, 'device': dev.alias, 'items': ids.length});
  }

  /// 新一轮发送开始前重置取消状态（UI 在 _sendAll 循环前调用）
  void resetSend() {
    _sendCancel = false;
    _sendRoots.clear();
  }

  /// 取消当前正在进行的发送：中断上传流并通知对端清理半成品
  Future<void> cancelCurrentSend() async {
    _sendCancel = true;
    await _notifyPeerCancel();
  }

  /// 向所有活动发送的对端发 HTTP cancel（尽力而为）
  Future<void> _notifyPeerCancel() async {
    final roots = List.of(_sendRoots);
    _sendRoots.clear();
    for (final r in roots) {
      final sep = r.indexOf('|');
      if (sep <= 0) continue;
      final base = r.substring(0, sep);
      final sid = r.substring(sep + 1);
      try {
        await _postWithTimeout('$base/cancel?sessionId=$sid', '', const Duration(seconds: 5));
      } catch (_) {}
    }
  }

  Future<void> sendText(Device dev, String text) async {
    if (text.trim().isEmpty) {
      sendDone.add({'ok': false, 'device': dev.alias, 'error': '文本为空'});
      return;
    }
    final tmp = File('${Directory.systemTemp.path}/qp_text_${_genUuid()}.txt');
    await tmp.writeAsString(text);
    try {
      await sendItems(dev, [
        SendItem(
          name: '剪贴板.txt',
          size: text.length,
          fileType: 'text/plain',
          localPath: tmp.path,
        ),
      ]);
    } finally {
      try { await tmp.delete(); } catch (_) {}
    }
  }

  Future<void> _uploadFile(String url, SendItem it) async {
    final file = File(it.localPath!);
    final length = await file.length();
    final request = await HttpClient()
        .postUrl(Uri.parse(url))
        .timeout(const Duration(seconds: 30));
    request.contentLength = length;
    request.headers.contentType = ContentType.binary;
    // 取消检查转发器：取消置位后在块间抛错 → addStream 失败 → 连接中断，对端清理半成品
    final stream = file.openRead().map((chunk) {
      if (_sendCancel) {
        throw StateError('send canceled by user');
      }
      return chunk;
    });
    try {
      await request.addStream(stream).timeout(const Duration(seconds: 300));
      final resp = await request.close().timeout(const Duration(seconds: 300));
      if (resp.statusCode != 200) {
        throw HttpException('HTTP ${resp.statusCode}');
      }
      await resp.drain();
    } catch (_) {
      // 中断连接（addStream 已被取消流打断）
      try { request.close(); } catch (_) {}
      rethrow;
    }
  }

  String _mapErr(Object e) {
    if (e is HttpException) return '数据传输失败（${e.message}）';
    if (e is SocketException) return '无法连接对端';
    if (e is TimeoutException) return '请求超时';
    return e.toString();
  }

  Future<String> _postWithTimeout(String url, String body, Duration timeout) async {
    final client = HttpClient()..connectionTimeout = const Duration(seconds: 10);
    try {
      final req = await client.postUrl(Uri.parse(url)).timeout(timeout);
      req.headers.contentType = ContentType.json;
      final bytes = utf8.encode(body);
      req.contentLength = bytes.length;
      req.add(bytes);
      final resp = await req.close().timeout(timeout);
      final text = await resp.transform(utf8.decoder).join().timeout(timeout);
      final code = resp.statusCode;
      if (code == 403) throw _SendDenied();
      if (code == 409) throw StateError('对端正忙');
      if (code != 200) throw HttpException('HTTP $code');
      return text;
    } finally {
      client.close(force: true);
    }
  }

  Future<void> _post(String url, Map<String, dynamic> body) async {
    await _postWithTimeout(url, jsonEncode(body), const Duration(seconds: 20));
  }

  Future<List<int>> _readBody(HttpRequest req) async {
    final bytes = <int>[];
    await for (final chunk in req) {
      bytes.addAll(chunk);
    }
    return bytes;
  }

  Map<String, dynamic>? _tryJson(String s) {
    try {
      return jsonDecode(s) as Map<String, dynamic>;
    } catch (_) {
      return null;
    }
  }

  void _json(HttpRequest req, int code, Map<String, dynamic> body) {
    req.response
      ..statusCode = code
      ..headers.contentType = ContentType.json
      ..write(jsonEncode(body))
      ..close();
  }

  void _text(HttpRequest req, int code, String s) {
    req.response
      ..statusCode = code
      ..headers.contentType = ContentType.text
      ..write(s)
      ..close();
  }

  static String _genUuid() {
    final r = Random.secure();
    final b = List<int>.generate(16, (_) => r.nextInt(256));
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    final h = b.map((x) => x.toRadixString(16).padLeft(2, '0')).join();
    return '${h.substring(0, 8)}-${h.substring(8, 12)}-${h.substring(12, 16)}-${h.substring(16, 20)}-${h.substring(20)}';
  }
}

class _SendDenied implements Exception {}

/// 剪贴板委托（避免依赖 flutter/services 在纯服务层）
class ClipboardDelegate {
  static void Function(String text) copy = (_) {};
}

/// 极简 SHA-256（dart 无内建，自实现；仅用于对端声明 sha256 时的完整性校验）
class Sha256Impl {
  final List<int> _h = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
    0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ];
  final List<int> _buf = [];
  int _total = 0;

  static const _k = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
  ];

  void update(List<int> data) {
    _buf.addAll(data);
    _total += data.length;
    while (_buf.length >= 64) {
      final chunk = _buf.sublist(0, 64);
      _buf.removeRange(0, 64);
      _process(chunk);
    }
  }

  void _process(List<int> chunk) {
    final w = List<int>.filled(64, 0);
    for (var i = 0; i < 16; i++) {
      w[i] = (chunk[i * 4] << 24) | (chunk[i * 4 + 1] << 16) | (chunk[i * 4 + 2] << 8) | chunk[i * 4 + 3];
    }
    for (var i = 16; i < 64; i++) {
      final s0 = _rotr(w[i - 15], 7) ^ _rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
      final s1 = _rotr(w[i - 2], 17) ^ _rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
      w[i] = (w[i - 16] + s0 + w[i - 7] + s1) & 0xffffffff;
    }
    var a = _h[0], b = _h[1], c = _h[2], d = _h[3];
    var e = _h[4], f = _h[5], g = _h[6], h = _h[7];
    for (var i = 0; i < 64; i++) {
      final s1 = _rotr(e, 6) ^ _rotr(e, 11) ^ _rotr(e, 25);
      final ch = (e & f) ^ ((~e) & g);
      final t1 = (h + s1 + ch + _k[i] + w[i]) & 0xffffffff;
      final s0 = _rotr(a, 2) ^ _rotr(a, 13) ^ _rotr(a, 22);
      final maj = (a & b) ^ (a & c) ^ (b & c);
      final t2 = (s0 + maj) & 0xffffffff;
      h = g; g = f; f = e;
      e = (d + t1) & 0xffffffff;
      d = c; c = b; b = a;
      a = (t1 + t2) & 0xffffffff;
    }
    _h[0] = (_h[0] + a) & 0xffffffff;
    _h[1] = (_h[1] + b) & 0xffffffff;
    _h[2] = (_h[2] + c) & 0xffffffff;
    _h[3] = (_h[3] + d) & 0xffffffff;
    _h[4] = (_h[4] + e) & 0xffffffff;
    _h[5] = (_h[5] + f) & 0xffffffff;
    _h[6] = (_h[6] + g) & 0xffffffff;
    _h[7] = (_h[7] + h) & 0xffffffff;
  }

  static int _rotr(int x, int n) => ((x >> n) | (x << (32 - n))) & 0xffffffff;

  String digestHex() {
    final lenBits = (_total * 8) & 0xffffffffffffffff;
    final padded = <int>[..._buf];
    padded.add(0x80);
    while (padded.length % 64 != 56) {
      padded.add(0);
    }
    for (var i = 7; i >= 0; i--) {
      padded.add((lenBits >> (i * 8)) & 0xff);
    }
    for (var i = 0; i + 64 <= padded.length; i += 64) {
      _process(padded.sublist(i, i + 64));
    }
    final sb = StringBuffer();
    for (final h in _h) {
      sb.write(h.toRadixString(16).padLeft(8, '0'));
    }
    return sb.toString();
  }
}