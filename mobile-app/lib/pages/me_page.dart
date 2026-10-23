// 我的页：常规（别名/服务器/界面主题）+ 文件传输 + 高级设置（折叠）+ 更多
import 'dart:convert';
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';
import 'package:url_launcher/url_launcher.dart';

import '../net/localsend.dart';
import '../theme.dart';
import '../widgets/neumorph.dart';

class MePage extends StatefulWidget {
  const MePage({super.key});

  @override
  State<MePage> createState() => _MePageState();
}

class _MePageState extends State<MePage> {
  final _svc = LocalSendService.instance;
  final _aliasCtrl = TextEditingController();
  bool _advOpen = false;
  int _cacheMb = 0;

  @override
  void initState() {
    super.initState();
    _aliasCtrl.text = _svc.alias;
    _cacheMb = (_svc.saveDir.isNotEmpty && Directory(_svc.saveDir).existsSync())
        ? (_dirSize(Directory(_svc.saveDir)) / 1024 / 1024).floor()
        : 0;
  }

  static int _dirSize(Directory dir) {
    var total = 0;
    try {
      for (final e in dir.listSync(recursive: true, followLinks: false)) {
        if (e is File) total += e.lengthSync();
      }
    } catch (_) {}
    return total;
  }

  void _saveAlias(String v) async {
    if (v.trim().isEmpty) {
      _aliasCtrl.text = _svc.alias;
      return;
    }
    _svc.alias = v.trim();
    _svc.bumpSettings();
    _svc.refresh();
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('alias', v.trim());
  }

  /// 界面主题当前显示值
  String _themeLabel() {
    switch (appThemeMode.value) {
      case ThemeMode.light:
        return '亮色';
      case ThemeMode.dark:
        return '暗色';
      default:
        return '跟随系统';
    }
  }

  Future<void> _setTheme(String v) async {
    final mode = v == '亮色' ? ThemeMode.light : v == '暗色' ? ThemeMode.dark : ThemeMode.system;
    applyThemeMode(mode); // 重赋全局 token + 广播（HomeShell 重建整页）
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('themeMode', v == '亮色' ? 'light' : v == '暗色' ? 'dark' : 'system');
    if (mounted) Toast.show(context, '界面主题已切换为「$v」');
  }

  Future<void> _setOnline(String v) async {
    setState(() => _svc.online = v == '在线');
    if (_svc.online) _svc.refresh();
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool('online', _svc.online);
    Toast.show(context, _svc.online ? '服务器已上线' : '服务器已离线');
  }

  Future<void> _setAutoAccept(bool v) async {
    setState(() => _svc.autoAccept = v);
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool('autoAccept', v);
  }

  Future<void> _pickSaveDir() async {
    final path = await FilePicker.getDirectoryPath();
    if (path == null) return;
    setState(() {
      _svc.saveDir = path;
      _cacheMb = (_dirSize(Directory(path)) / 1024 / 1024).floor();
    });
    final prefs = await SharedPreferences.getInstance();
    await prefs.setString('saveDir', path);
    Toast.show(context, '保存目录已更新');
  }

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(16, 4, 16, 104),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // ───── 常规 ─────
          NeoCard(
            Column(
              children: [
                const Align(alignment: Alignment.centerLeft, child: NeoCardHeader(Icons.tune_rounded, '常规')),
                NeoRow(
                  isFirst: true,
                  left: const NeoRowTitle('别名'),
                  right: NeoField(controller: _aliasCtrl, width: 118, onChanged: _saveAlias),
                ),
                NeoRow(
                  left: const NeoRowTitle('服务器'),
                  right: Row(
                    children: [
                      NeoSelect(_svc.online ? '在线' : '离线', const ['在线', '离线'], width: 90, onChanged: (v) => _setOnline(v)),
                      const SizedBox(width: 8),
                      NeoButton('刷新', small: true, onTap: () {
                        _svc.refresh();
                        Toast.show(context, '已重新广播');
                      }),
                    ],
                  ),
                ),
                NeoRow(
                  left: const NeoRowTitle('界面主题'),
                  right: NeoSelect(_themeLabel(), const ['亮色', '暗色', '跟随系统'], width: 118, onChanged: _setTheme),
                ),
              ],
            ),
          ),
          // ───── 文件传输 ─────
          NeoCard(
            Column(
              children: [
                const Align(alignment: Alignment.centerLeft, child: NeoCardHeader(Icons.folder_shared_outlined, '文件传输')),
                NeoRow(
                  isFirst: true,
                  left: const NeoRowTitle('快速保存'),
                  right: NeoSwitch(value: _svc.autoAccept, onChanged: _setAutoAccept),
                ),
                NeoRow(
                  left: const NeoRowTitle('保存目录'),
                  right: Row(
                    children: [
                      Container(
                        width: 120,
                        height: 36,
                        padding: const EdgeInsets.symmetric(horizontal: 10),
                        decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(11), boxShadow: kInSm),
                        alignment: Alignment.centerLeft,
                        child: Text(
                          _svc.saveDir.isEmpty ? 'Download/QuickCopy' : _svc.saveDir,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: TextStyle(fontSize: 11, color: kFg),
                        ),
                      ),
                      const SizedBox(width: 8),
                      NeoButton('选择…', small: true, onTap: _pickSaveDir),
                    ],
                  ),
                ),
                NeoRow(
                  left: const NeoRowTitle('仅通过 Wi-Fi 接收'),
                  right: NeoSwitch(value: _svc.wifiOnly, onChanged: (v) {
                    setState(() => _svc.wifiOnly = v);
                    SharedPreferences.getInstance().then((p) => p.setBool('wifiOnly', v));
                  }),
                ),
                NeoRow(
                  left: const NeoRowTitle('震动提示接收'),
                  right: NeoSwitch(value: _svc.vibrate, onChanged: (v) {
                    setState(() => _svc.vibrate = v);
                    SharedPreferences.getInstance().then((p) => p.setBool('vibrate', v));
                  }),
                ),
              ],
            ),
          ),
          // ───── 高级设置（折叠） ─────
          NeoCard(
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const Align(alignment: Alignment.centerLeft, child: NeoCardHeader(Icons.settings_outlined, '高级设置')),
                GestureDetector(
                  onTap: () => setState(() => _advOpen = !_advOpen),
                  child: Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: Row(
                      children: [
                        Text(_advOpen ? '收起高级设置' : '展开高级设置', style: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: kAcc)),
                        const Spacer(),
                        AnimatedRotation(
                          turns: _advOpen ? 0.5 : 0,
                          duration: const Duration(milliseconds: 200),
                          child: Icon(Icons.keyboard_arrow_down, size: 18, color: kAcc),
                        ),
                      ],
                    ),
                  ),
                ),
                if (_advOpen)
                  Column(
                    children: [
                      NeoRow(
                        isFirst: true,
                        left: const NeoRowTitle('监听端口'),
                        right: Container(
                          padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 8),
                          decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(11), boxShadow: kInSm),
                          child: Text('53318', style: TextStyle(fontSize: 13.5, fontWeight: FontWeight.w600, color: kFg)),
                        ),
                      ),
                      NeoRow(
                        left: const NeoRowTitle('搜索设备超时'),
                        right: NeoField(
                          controller: TextEditingController(text: '25'),
                          width: 118,
                          keyboardType: TextInputType.number,
                          onChanged: (_) {},
                        ),
                      ),
                      NeoRow(
                        left: const NeoRowTitle('缓存'),
                        right: Row(
                          children: [
                            Text('$_cacheMb MB', style: TextStyle(fontSize: 13.5, color: kFg, fontWeight: FontWeight.w600)),
                            const SizedBox(width: 8),
                            NeoButton('打开目录', small: true, onTap: _openCacheDir),
                            const SizedBox(width: 8),
                            NeoButton('清空缓存', small: true, onTap: _clearCache),
                          ],
                        ),
                      ),
                    ],
                  ),
              ],
            ),
          ),
          // ───── 更多 ─────
          NeoCard(
            Column(
              children: [
                const Align(alignment: Alignment.centerLeft, child: NeoCardHeader(Icons.more_horiz_rounded, '更多')),
                NeoRow(
                  isFirst: true,
                  left: const NeoRowTitle('关于'),
                  right: _mBtn('打开', _aboutDialog),
                ),
                NeoRow(
                  left: const NeoRowTitle('支持', sub: '认可 QuickCopy ？打赏支持一下'),
                  right: _mBtn('打赏', () => Toast.show(context, '感谢支持')),
                ),
                NeoRow(
                  left: const NeoRowTitle('检查更新'),
                  right: _mBtn('检测', _checkUpdate),
                ),
                NeoRow(
                  left: const NeoRowTitle('更新日志'),
                  right: _mBtn(kAppVersion, _changelogDialog),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _mBtn(String text, VoidCallback onTap) {
    return SizedBox(
      width: 76,
      child: NeoButton(text, small: true, onTap: onTap),
    );
  }

  void _openCacheDir() {
    final dir = _svc.saveDir;
    if (dir.isEmpty || !Directory(dir).existsSync()) {
      Toast.show(context, '保存目录不存在');
      return;
    }
    Process.start('explorer', [dir]);
  }

  void _clearCache() async {
    final dir = Directory(_svc.saveDir);
    if (!dir.existsSync()) {
      Toast.show(context, '保存目录不存在');
      return;
    }
    try {
      for (final e in dir.listSync(recursive: true, followLinks: false)) {
        if (e is File) e.deleteSync();
      }
      if (mounted) {
        setState(() => _cacheMb = 0);
        Toast.show(context, '缓存已清空');
      }
    } catch (_) {
      if (mounted) Toast.show(context, '清空失败');
    }
  }

  void _aboutDialog() {
    showDialog(
      context: context,
      builder: (ctx) => Dialog(
        backgroundColor: kBg,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(Icons.bolt, size: 40, color: kAcc),
              const SizedBox(height: 8),
              Text('QuickCopy', style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600, color: kFg)),
              const SizedBox(height: 4),
              Text('v$kAppVersion · 与电脑端互通', style: TextStyle(fontSize: 12.5, color: kMut)),
              const SizedBox(height: 6),
              Text('局域网文件快速互传\n（LocalSend v2 协议兼容）', textAlign: TextAlign.center, style: TextStyle(fontSize: 12.5, color: kMut, height: 1.6)),
              const SizedBox(height: 14),
              NeoButton('好的', small: true, onTap: () => Navigator.pop(ctx)),
            ],
          ),
        ),
      ),
    );
  }

  /// 检查更新：请求 GitHub Releases 最新版本，与当前版本比较后提示
  Future<void> _checkUpdate() async {
    Toast.show(context, '正在检查更新…');
    try {
      final client = HttpClient()..connectionTimeout = const Duration(seconds: 10);
      final req = await client
          .getUrl(Uri.parse('https://api.github.com/repos/SealSkyA/QuickPast/releases/latest'));
      req.headers.set('User-Agent', 'QuickCopy');
      req.headers.set('Accept', 'application/vnd.github+json');
      final resp = await req.close();
      final body = await resp.transform(const Utf8Decoder()).join();
      client.close();
      if (resp.statusCode != 200) throw Exception('http ${resp.statusCode}');
      final latest = ((jsonDecode(body)['tag_name'] as String?) ?? '')
          .replaceFirst(RegExp(r'^v'), '');
      if (latest.isEmpty) throw Exception('no tag');
      if (_cmpVersion(latest, kAppVersion) > 0) {
        if (mounted) _showUpdateDialog(latest);
      } else {
        if (mounted) Toast.show(context, '已是最新版本 $kAppVersion');
      }
    } catch (_) {
      if (mounted) Toast.show(context, '检查更新失败，请检查网络');
    }
  }

  /// 语义版本比较：a > b 返回 1，相等 0，否则 -1
  static int _cmpVersion(String a, String b) {
    final av = a.split('.').map((e) => int.tryParse(e) ?? 0).toList();
    final bv = b.split('.').map((e) => int.tryParse(e) ?? 0).toList();
    for (var i = 0; i < (av.length > bv.length ? av.length : bv.length); i++) {
      final x = i < av.length ? av[i] : 0;
      final y = i < bv.length ? bv[i] : 0;
      if (x != y) return x > y ? 1 : -1;
    }
    return 0;
  }

  void _showUpdateDialog(String latest) {
    showDialog(
      context: context,
      builder: (ctx) => Dialog(
        backgroundColor: kBg,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('发现新版本 v$latest', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600, color: kFg)),
              const SizedBox(height: 10),
              Text('当前版本 $kAppVersion\n最新版本 v$latest\n\n可到 GitHub Releases 下载最新 APK 安装包。',
                  style: TextStyle(fontSize: 12.5, color: kMut, height: 1.6)),
              const SizedBox(height: 14),
              Row(
                children: [
                  Expanded(
                    child: NeoButton('去下载', primary: true, onTap: () {
                      Navigator.pop(ctx);
                      _openReleasePage();
                    }),
                  ),
                  const SizedBox(width: 10),
                  Expanded(child: NeoButton('以后再说', onTap: () => Navigator.pop(ctx))),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  /// 打开 GitHub Releases 下载页；失败则复制链接兜底
  Future<void> _openReleasePage() async {
    const url = 'https://github.com/SealSkyA/QuickPast/releases';
    try {
      await launchUrl(Uri.parse(url), mode: LaunchMode.externalApplication);
    } catch (_) {
      copyToClipboard(url);
      if (mounted) Toast.show(context, '打开失败，链接已复制');
    }
  }

  void _changelogDialog() {
    showDialog(
      context: context,
      builder: (ctx) => Dialog(
        backgroundColor: kBg,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Text('更新日志', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
              const SizedBox(height: 10),
              Text('v0.2.15 · 品牌更名 QuickCopy、图标改色、支持在线检查更新\nv0.2.13 · 手机端初版，支持收发文件与文本',
                  style: TextStyle(fontSize: 12.5, color: kMut, height: 1.7)),
            ],
          ),
        ),
      ),
    );
  }
}