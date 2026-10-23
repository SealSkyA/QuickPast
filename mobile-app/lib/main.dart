// QuickCopy 手机端 · 入口
import 'dart:io' show Platform;

import 'package:device_info_plus/device_info_plus.dart';
import 'package:flutter/material.dart';
import 'package:permission_handler/permission_handler.dart';
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

  await _requestStoragePermission(); // 首次启动弹存储授权，保证接收文件落盘到 Download
  runApp(const QuickCopyApp());
}

/// 存储授权：Android ≤12 请求读写存储；Android 11+ 分区存储下直写
/// Download 需要「所有文件访问」（MANAGE_EXTERNAL_STORAGE，跳系统设置页）。
Future<void> _requestStoragePermission() async {
  if (!Platform.isAndroid) return;
  try {
    final info = await DeviceInfoPlugin().androidInfo;
    if (info.version.sdkInt >= 30) {
      if (!await Permission.manageExternalStorage.isGranted) {
        // Android 11+ 的 MANAGE_EXTERNAL_STORAGE 系统不弹授权对话框，
        // request() 会直接返回，需跳转应用设置页引导用户手动开启「所有文件访问」
        await Permission.manageExternalStorage.request();
        if (!await Permission.manageExternalStorage.isGranted) {
          await openAppSettings();
        }
      }
    } else {
      if (!await Permission.storage.isGranted) {
        await Permission.storage.request();
      }
    }
  } catch (_) {} // 无权限对话框环境（如部分模拟器）不阻塞启动
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

class QuickCopyApp extends StatefulWidget {
  const QuickCopyApp({super.key});

  @override
  State<QuickCopyApp> createState() => _QuickCopyAppState();
}

class _QuickCopyAppState extends State<QuickCopyApp> {
  ThemeMode _mode = ThemeMode.system;

  @override
  void initState() {
    super.initState();
    _loadTheme();
  }

  /// 读取持久化的主题模式并应用（更新全局 token + MaterialApp.themeMode）
  Future<void> _loadTheme() async {
    final prefs = await SharedPreferences.getInstance();
    final s = prefs.getString('themeMode') ?? 'system';
    final mode = s == 'light'
        ? ThemeMode.light
        : s == 'dark'
            ? ThemeMode.dark
            : ThemeMode.system;
    applyThemeMode(mode);
    if (mounted) setState(() => _mode = mode);
  }

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'QuickCopy',
      debugShowCheckedModeBanner: false,
      theme: buildNeumorphTheme(),
      darkTheme: buildNeumorphDarkTheme(),
      themeMode: _mode,
      home: const HomeShell(),
    );
  }
}