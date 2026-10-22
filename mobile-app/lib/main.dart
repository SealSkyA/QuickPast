// QuickPaste 手机端 · 入口
import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

import 'net/localsend.dart';
import 'pages/home_shell.dart';
import 'theme.dart';
import 'widgets/neumorph.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await _loadSettings();
  // 剪贴板委托：网络层收到文本/发送剪贴板时调用
  ClipboardDelegate.copy = copyToClipboard;

  final svc = LocalSendService.instance;
  svc.start(); // 后台启动发现 + HTTP 服务

  runApp(const QuickPasteApp());
}

Future<void> _loadSettings() async {
  final prefs = await SharedPreferences.getInstance();
  final svc = LocalSendService.instance;
  svc.alias = prefs.getString('alias') ?? '我的手机';
  svc.fingerprint = prefs.getString('fingerprint') ?? '';
  svc.online = prefs.getBool('online') ?? true;
  svc.autoAccept = prefs.getBool('autoAccept') ?? false;
  svc.saveDir = prefs.getString('saveDir') ?? '';
  svc.wifiOnly = prefs.getBool('wifiOnly') ?? true;
  svc.vibrate = prefs.getBool('vibrate') ?? true;
  await svc.initFingerprint(() async => prefs.getString('fingerprint') ?? '');
  await prefs.setString('fingerprint', svc.fingerprint);
}

class QuickPasteApp extends StatelessWidget {
  const QuickPasteApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'QuickPaste',
      debugShowCheckedModeBanner: false,
      theme: buildNeumorphTheme(),
      home: const HomeShell(),
    );
  }
}