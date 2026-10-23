// 打开文件管理（Android 原生桥）：FileProvider + ACTION_VIEW 打开目录
import 'package:flutter/services.dart';

const MethodChannel _open = MethodChannel('qp_open');

/// 调用系统文件管理器打开目录；失败（无文件管理器/路径无效）返回 false
Future<bool> openDirInFiles(String path) async {
  try {
    final ok = await _open.invokeMethod('openDir', path);
    return ok == true;
  } catch (_) {
    return false;
  }
}
