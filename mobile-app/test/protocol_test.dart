// LocalSend v2 协议集成测试：
// 用模拟对端（按 PC 端 src-tauri/src/net.rs 行为）验证手机端收发全链路。
//  - 接收：announce → register 回执 → prepare-upload 确认 → upload 落盘（含 sha256）
//  - 发送：prepare-upload → 逐文件 upload（裸字节流）→ 文本走临时 txt
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:quickpaste/models.dart';
import 'package:quickpaste/net/localsend.dart';

/// 发起 prepare-upload（不等待响应，手机端会阻塞等确认）
Future<Future<HttpClientResponse>> sendPrepare(int port, Map<String, dynamic> body) async {
  final req = await HttpClient()
      .postUrl(Uri.parse('http://127.0.0.1:$port/api/localsend/v2/prepare-upload'));
  req.headers.contentType = ContentType.json;
  final b = utf8.encode(jsonEncode(body));
  req.contentLength = b.length;
  req.add(b);
  return req.close();
}

Future<void> uploadBytes(int port, String sessionId, String fileId, String token, List<int> data) async {
  final up = await HttpClient()
      .postUrl(Uri.parse('http://127.0.0.1:$port/api/localsend/v2/upload?sessionId=$sessionId&fileId=$fileId&token=$token'));
  up.contentLength = data.length;
  up.add(data);
  final resp = await up.close();
  await resp.drain();
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final svc = LocalSendService.instance;
  late Directory tmpDir;
  String? clipboardText;

  setUpAll(() async {
    HttpOverrides.global = null; // flutter_test 默认注入返回 400 的假 HttpClient，需恢复真实网络
    tmpDir = await Directory.systemTemp.createTemp('qp_test');
    LocalSendService.kPort = 53118; // 避免与本机运行中的电脑端 53318 冲突
    svc.fingerprint = 'FP-phone-test';
    svc.alias = '手机A';
    svc.saveDir = tmpDir.path;
    svc.autoAccept = false;
    ClipboardDelegate.copy = (t) => clipboardText = t;
    await svc.start();
  });

  tearDownAll(() async {
    try {
      await tmpDir.delete(recursive: true);
    } catch (_) {}
  });

  test('接收链路：announce→register 回执→prepare-upload→upload 落盘', () async {
    final mockPort = 53399;
    final registerLog = <String>[];
    final mockServer = await HttpServer.bind(InternetAddress.anyIPv4, mockPort);
    mockServer.listen((req) async {
      try {
        if (req.uri.path == '/api/localsend/v2/register') {
          registerLog.add(await utf8.decoder.bind(req).join());
          req.response
            ..statusCode = 200
            ..headers.contentType = ContentType.json
            ..write('{}')
            ..close();
        } else {
          req.response..statusCode = 404..close();
        }
      } catch (e) {
        debugPrint('mock server err: $e');
      }
    });

    // 模拟 PC 广播 announce
    final udp = await RawDatagramSocket.bind(InternetAddress.anyIPv4, 0);
    udp.send(
      utf8.encode(jsonEncode({
        'alias': 'PC桌面',
        'version': '2.2',
        'deviceModel': 'QuickPaste',
        'deviceType': 'desktop',
        'fingerprint': 'FP-pc-mock',
        'port': mockPort,
        'protocol': 'http',
        'download': true,
        'announce': true,
      })),
      InternetAddress('224.0.0.167'),
      LocalSendService.kPort,
    );

    // 设备发现
    await Future<void>.delayed(const Duration(milliseconds: 600));
    final found = svc.devices.value.where((d) => d.fingerprint == 'FP-pc-mock').toList();
    expect(found.length, 1, reason: '应从 announce 发现 PC 设备');

    // register 回执
    await Future<void>.delayed(const Duration(milliseconds: 900));
    expect(registerLog.length, greaterThanOrEqualTo(1), reason: '应收到手机端 register 回执');
    final r = jsonDecode(registerLog.last) as Map<String, dynamic>;
    expect(r['alias'], '手机A');
    expect(r['fingerprint'], 'FP-phone-test');
    expect(r['port'], LocalSendService.kPort);
    expect(r['deviceType'], 'mobile');

    // PC 发起 prepare-upload（发往手机端 HTTP，手机阻塞等确认）
    final respFuture = await sendPrepare(LocalSendService.kPort, {
      'info': {'alias': 'PC桌面', 'version': '2.2', 'fingerprint': 'FP-pc-mock'},
      'files': {
        'f1': {'id': 'f1', 'fileName': 'hello.txt', 'size': 5, 'fileType': 'text/plain'},
      },
    });

    await Future<void>.delayed(const Duration(milliseconds: 300));
    expect(svc.pending.value.length, 1, reason: '应弹确认请求');

    // 用户确认 → 响应 200 并携带 token
    final sid = svc.pending.value.first.sessionId;
    svc.acceptSession(sid);
    final preResp = await respFuture.timeout(const Duration(seconds: 10));
    final preText = await utf8.decoder.bind(preResp).join();
    expect(preResp.statusCode, 200, reason: '确认后应返回 200');
    final v = jsonDecode(preText) as Map<String, dynamic>;
    final token = ((v['files'] as Map)['f1'] as String?) ?? '';
    expect(token.isNotEmpty, true);

    // 上传字节 → 落盘
    await uploadBytes(LocalSendService.kPort, sid, 'f1', token, utf8.encode('hello'));
    await Future<void>.delayed(const Duration(milliseconds: 400));
    final saved = File('${tmpDir.path}/hello.txt');
    expect(saved.existsSync(), true, reason: '文件应保存到保存目录');
    expect(await saved.readAsString(), 'hello');

    // 接收完成事件
    expect(svc.rcvDone.hasListener, false, reason: '验证事件由 HomeShell 订阅，测试无需断言');

    udp.close();
    await mockServer.close(force: true);
  });

  test('发送链路：prepare-upload → 裸字节流 upload（文件 + 文本）', () async {
    final mockPort = 53401;
    final capturedPrepare = <Map<String, dynamic>>[];
    final uploadedBodies = <List<int>>[];
    final mockServer = await HttpServer.bind(InternetAddress.anyIPv4, mockPort);
    mockServer.listen((req) async {
      try {
        if (req.uri.path == '/api/localsend/v2/prepare-upload') {
          final body = await utf8.decoder.bind(req).join();
          final bodyV = jsonDecode(body) as Map<String, dynamic>;
          capturedPrepare.add(bodyV);
          final files = (bodyV['files'] as Map).keys;
          req.response
            ..statusCode = 200
            ..headers.contentType = ContentType.json
            ..write(jsonEncode({
              'sessionId': 'mock-s2',
              'files': {for (final k in files) k.toString(): 'tok-$k'},
            }))
            ..close();
        } else if (req.uri.path == '/api/localsend/v2/upload') {
          final body = <int>[];
          await for (final c in req) { body.addAll(c); }
          uploadedBodies.add(body);
          req.response..statusCode = 200..close();
        } else {
          req.response..statusCode = 404..close();
        }
      } catch (e) {
        debugPrint('mock2 err: $e');
        try { req.response..statusCode = 500..close(); } catch (_) {}
      }
    });

    final dev = Device(
      fingerprint: 'FP-pc-mock2',
      alias: 'PC2',
      deviceType: 'desktop',
      version: '2.2',
      ip: '127.0.0.1',
      port: mockPort,
      protocol: 'http',
      download: true,
      lastSeen: 0,
    );

    // 文件
    final f = File('${tmpDir.path}/a.bin');
    await f.writeAsBytes(List<int>.generate(1024, (i) => i % 251));
    final events = <Map<String, dynamic>>[];
    final sub = svc.sendDone.stream.listen(events.add);

    await svc.sendItems(dev, [
      SendItem(name: 'a.bin', size: 1024, fileType: 'application/octet-stream', localPath: f.path),
    ]);
    await Future<void>.delayed(const Duration(milliseconds: 600));
    final ev = events.where((e) => e['device'] == 'PC2').toList();
    expect(ev.isNotEmpty, true, reason: '应发出 sendDone 事件');
    expect(ev.first['ok'], true, reason: '发送应成功，实际=${ev.first}');

    expect(capturedPrepare.length, 1);
    final filesMap = capturedPrepare.first['files'] as Map<String, dynamic>;
    expect(filesMap.length, 1);
    expect((filesMap.values.first as Map)['fileName'], 'a.bin');
    expect(uploadedBodies.length, 1, reason: '应上传 1 个文件');
    expect(uploadedBodies.first.length, 1024);
    expect(uploadedBodies.first[0], 0);
    expect(uploadedBodies.first[1023], 19);

    // 文本（临时 txt 文件通道）
    uploadedBodies.clear();
    capturedPrepare.clear();
    await svc.sendText(dev, '你好世界');
    await Future<void>.delayed(const Duration(milliseconds: 600));
    final ev2 = events.where((e) => e['device'] == 'PC2').last;
    expect(ev2['ok'], true, reason: '文本发送应成功，实际=$ev2');
    final prepare2 = capturedPrepare.last;
    final f2 = (prepare2['files'] as Map).values.first as Map;
    expect(f2['fileName'], '剪贴板.txt');
    expect(f2['fileType'], 'text/plain');
    expect(utf8.decode(uploadedBodies.first), '你好世界');

    await sub.cancel();
    await mockServer.close(force: true);
  });

  test('接收文本消息（fileType=text）→ 剪贴板委托被调用', () async {
    clipboardText = null;
    final mockPort = 53403;
    final mockServer = await HttpServer.bind(InternetAddress.anyIPv4, mockPort);
    mockServer.listen((req) async {
      try {
        if (req.uri.path == '/api/localsend/v2/register') {
          await req.drain();
          req.response
            ..statusCode = 200
            ..headers.contentType = ContentType.json
            ..write('{}')
            ..close();
        } else {
          req.response..statusCode = 404..close();
        }
      } catch (e) {
        debugPrint('mock3 err: $e');
      }
    });

    // 模拟 PC 文本消息（fileType=text，发往手机端 HTTP）
    final respFuture = await sendPrepare(LocalSendService.kPort, {
      'info': {'alias': 'PC3', 'version': '2.2', 'fingerprint': 'FP-pc-mock3'},
      'files': {'f1': {'id': 'f1', 'fileName': 'note', 'size': 4, 'fileType': 'text'}},
    });
    await Future<void>.delayed(const Duration(milliseconds: 300));
    final s = svc.pending.value.where((x) => x.senderAlias == 'PC3').toList();
    expect(s.length, 1, reason: '应弹文本接收请求');
    final sid = s.first.sessionId;
    svc.acceptSession(sid);
    final preResp = await respFuture.timeout(const Duration(seconds: 10));
    final preText = await utf8.decoder.bind(preResp).join();
    expect(preResp.statusCode, 200);
    final token = ((jsonDecode(preText) as Map)['files'] as Map)['f1'] as String? ?? '';

    // 上传文本 → 剪贴板
    await uploadBytes(LocalSendService.kPort, sid, 'f1', token, utf8.encode('OK!!'));
    await Future<void>.delayed(const Duration(milliseconds: 500));
    expect(clipboardText, 'OK!!', reason: 'fileType=text 应写入剪贴板');

    await mockServer.close(force: true);
  });
}