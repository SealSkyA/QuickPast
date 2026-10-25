// 发送页（对齐 UI 原型）：发送类型四宫格 + 待发列表 + 附近设备（搜索/工具/勾选 → 批量发送）
import 'dart:io';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../models.dart';
import '../net/localsend.dart';
import '../theme.dart';
import '../widgets/neumorph.dart';

class SendPage extends StatefulWidget {
  const SendPage({super.key});

  @override
  State<SendPage> createState() => _SendPageState();
}

class _SendPageState extends State<SendPage> {
  final _svc = LocalSendService.instance;
  final List<SendItem> _pending = [];
  final Set<String> _checked = {}; // fingerprint
  bool _onlyDownload = false;
  bool _onlyFav = false;
  final _searchCtrl = TextEditingController();
  String _q = '';

  static String _mimeOf(String name) {
    final ext = name.contains('.') ? name.split('.').last.toLowerCase() : '';
    switch (ext) {
      case 'txt': case 'md': case 'log': case 'json': return 'text/plain';
      case 'png': return 'image/png';
      case 'jpg': case 'jpeg': return 'image/jpeg';
      case 'gif': return 'image/gif';
      case 'webp': return 'image/webp';
      case 'pdf': return 'application/pdf';
      case 'zip': return 'application/zip';
      default: return 'application/octet-stream';
    }
  }

  @override
  void dispose() {
    _searchCtrl.dispose();
    super.dispose();
  }

  Future<void> _pickFile() async {
    final files = await FilePicker.pickFiles(type: FileType.any);
    if (files.isEmpty) return;
    setState(() {
      for (final f in files) {
        _pending.add(SendItem(
          name: f.name,
          size: f.lengthSync() ?? 0,
          fileType: _mimeOf(f.name),
          localPath: f.path,
        ));
      }
    });
  }

  Future<void> _pickFolder() async {
    final path = await FilePicker.getDirectoryPath();
    if (path == null) return;
    final dir = Directory(path);
    final files = <File>[];
    await for (final e in dir.list(recursive: true, followLinks: false)) {
      if (e is File) files.add(e);
    }
    if (files.isEmpty) {
      Toast.show(context, '该文件夹中没有文件');
      return;
    }
    setState(() {
      for (final f in files) {
        final rel = f.path.substring(path.length).replaceAll('\\', '/').replaceFirst('/', '');
        _pending.add(SendItem(
          name: rel,
          size: f.lengthSync(),
          fileType: _mimeOf(f.path),
          localPath: f.path,
        ));
      }
    });
  }

  Future<void> _pickText() async {
    final ctrl = TextEditingController();
    final result = await showDialog<String>(
      context: context,
      builder: (ctx) => Dialog(
        backgroundColor: kBg,
        shape: RoundedRectangleBorder(borderRadius: BorderRadius.circular(16)),
        child: Padding(
          padding: const EdgeInsets.all(20),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text('发送文本', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600, color: kFg)),
              const SizedBox(height: 12),
              NeoField(controller: ctrl, width: double.infinity, hint: '输入要发送的文本…', paddingV: 10),
              const SizedBox(height: 16),
              Row(
                mainAxisAlignment: MainAxisAlignment.end,
                children: [
                  NeoButton('取消', small: true, onTap: () => Navigator.pop(ctx)),
                  const SizedBox(width: 10),
                  NeoButton('发送', small: true, primary: true, onTap: () => Navigator.pop(ctx, ctrl.text)),
                ],
              ),
            ],
          ),
        ),
      ),
    );
    if (result == null || result.trim().isEmpty) return;
    setState(() {
      _pending.add(SendItem(
        name: '剪贴板.txt',
        size: result.length,
        fileType: 'text/plain',
        text: result,
      ));
    });
  }

  Future<void> _pickClipboard() async {
    final data = await Clipboard.getData('text/plain');
    final text = data?.text ?? '';
    if (text.trim().isEmpty) {
      Toast.show(context, '系统剪贴板为空');
      return;
    }
    setState(() {
      _pending.add(SendItem(
        name: '剪贴板.txt',
        size: text.length,
        fileType: 'text/plain',
        text: text,
      ));
    });
  }

  void _removeItem(int i) {
    setState(() => _pending.removeAt(i));
  }

  void _toggleDevice(String fp) {
    setState(() {
      if (!_checked.remove(fp)) _checked.add(fp);
    });
  }

  void _toggleFav(String fp) {
    _svc.toggleFav(fp);
    setState(() {});
  }

  Future<void> _sendAll() async {
    if (_pending.isEmpty) {
      Toast.show(context, '先在上方选择要发送的内容');
      return;
    }
    final devs = _svc.devices.value.where((d) => _checked.contains(d.fingerprint)).toList();
    if (devs.isEmpty) {
      Toast.show(context, '请先在附近设备中勾选目标设备');
      return;
    }
    _svc.resetSend();
    for (final d in devs) {
      _svc.sendItems(d, List.of(_pending));
    }
    Toast.show(context, '正在发送给 ${devs.length} 台设备…');
  }

  void _toggleAll() {
    final devs = _svc.devices.value;
    setState(() {
      if (_checked.length == devs.length && devs.isNotEmpty) {
        _checked.clear();
      } else {
        _checked
          ..clear()
          ..addAll(devs.map((d) => d.fingerprint));
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(16, 4, 16, 104),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // ───── 发送卡 ─────
          NeoCard(
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const NeoCardHeader(Icons.send_rounded, '发送'),
                const SizedBox(height: 4),
                Padding(
                  padding: EdgeInsets.only(bottom: 12),
                  child: Text('先选择发送类型，再勾选附近设备；发送模式决定目标与行为。', style: TextStyle(fontSize: 12, color: kMut)),
                ),
                // 类型四宫格
                Row(
                  children: [
                    _stype(Icons.insert_drive_file_outlined, '文件', _pickFile),
                    const SizedBox(width: 10),
                    _stype(Icons.folder_outlined, '文件夹', _pickFolder),
                    const SizedBox(width: 10),
                    _stype(Icons.notes_outlined, '文本', _pickText),
                    const SizedBox(width: 10),
                    _stype(Icons.content_paste_outlined, '粘贴板', _pickClipboard),
                  ],
                ),
                // 待发列表 / 未选择提示
                const SizedBox(height: 12),
                if (_pending.isEmpty)
                  Text('未选择内容 — 点击上方类型图标挑选要发送的内容', style: TextStyle(fontSize: 12, color: kMut))
                else
                  Column(
                    children: [
                      for (var i = 0; i < _pending.length; i++) _pendingRow(i),
                    ],
                  ),
              ],
            ),
          ),
          // ───── 附近设备卡 ─────
          NeoCard(
            Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // 标题 + 右侧工具（对齐原型）
                Row(
                  children: [
                    Icon(Icons.radar_rounded, size: 17, color: kAcc),
                    const SizedBox(width: 9),
                    Expanded(child: Text('附近设备', style: TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600, color: kFg))),
                    _tool(Icons.check_box_outlined, '全选/清空', _toggleAll),
                    const SizedBox(width: 2),
                    _tool(
                      _onlyFav ? Icons.star_rounded : Icons.star_border_rounded,
                      _onlyFav ? '收起只看收藏' : '只看收藏设备',
                      () => setState(() => _onlyFav = !_onlyFav),
                      color: _onlyFav ? kAcc : kMut,
                    ),
                    const SizedBox(width: 2),
                    _tool(Icons.phonelink_rounded, '仅看可接收设备', () => setState(() => _onlyDownload = !_onlyDownload)),
                    const SizedBox(width: 2),
                    _tool(Icons.more_horiz_rounded, '更多', () => Toast.show(context, '更多选项见「我的」页')),
                  ],
                ),
                const SizedBox(height: 4),
                Padding(
                  padding: EdgeInsets.only(bottom: 8),
                  child: Text('发送模式：一个接收者 · 发送后自动取消选择', style: TextStyle(fontSize: 12, color: kMut)),
                ),
                // 搜索框
                NeoSearchBox(
                  controller: _searchCtrl,
                  hint: '搜索设备名称 / 系统…',
                  onChanged: (v) => setState(() => _q = v.trim().toLowerCase()),
                ),
                const SizedBox(height: 2),
                // 设备列表
                ValueListenableBuilder<List<Device>>(
                  valueListenable: _svc.devices,
                  builder: (_, devs, __) {
                    final list = devs
                        .where((d) => !_onlyFav || _svc.isFav(d.fingerprint))
                        .where((d) => _onlyDownload ? (d.download || d.deviceType == 'mobile') : true)
                        .where((d) => _q.isEmpty || d.alias.toLowerCase().contains(_q) || d.deviceType.toLowerCase().contains(_q))
                        .toList();
                    if (list.isEmpty) {
                      return Container(
                        width: double.infinity,
                        padding: const EdgeInsets.symmetric(vertical: 26),
                        child: Column(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            Icon(_onlyFav ? Icons.star_border_rounded : Icons.wifi_tethering, size: 34, color: kMut),
                            SizedBox(height: 8),
                            Text(_onlyFav ? '没有收藏的设备\n可在设备右侧点星标收藏' : '未发现附近设备\n请确认对方已开启 QuickCopy',
                                textAlign: TextAlign.center, style: TextStyle(fontSize: 12.5, color: kMut, height: 1.6)),
                          ],
                        ),
                      );
                    }
                    return Column(
                      children: [
                        for (var i = 0; i < list.length; i++) ...[
                          if (i > 0) Divider(color: kLt, height: 1),
                          _deviceRow(list[i]),
                        ],
                      ],
                    );
                  },
                ),
                // devbar（刻痕分隔）
                Container(
                  margin: const EdgeInsets.only(top: 10),
                  padding: const EdgeInsets.only(top: 10),
                  decoration: BoxDecoration(
                    border: Border(top: BorderSide(color: kLt, width: 1)),
                  ),
                  constraints: const BoxConstraints(minHeight: 38),
                  child: Column(
                    children: [
                      Row(
                        children: [
                          Expanded(
                            child: Text(
                              '已选 ${_checked.length} 台设备',
                              style: TextStyle(fontSize: 12, color: kMut),
                            ),
                          ),
                          NeoButton('发送', primary: true, small: true, onTap: _sendAll),
                          const SizedBox(width: 8),
                          NeoButton('取消', small: true, onTap: () => setState(_checked.clear)),
                        ],
                      ),
                      // 发送进度 + 取消发送（发送中显示）
                      ValueListenableBuilder<Map<String, dynamic>?>(
                        valueListenable: _svc.sendProgress,
                        builder: (_, sp, __) {
                          if (sp == null) return const SizedBox.shrink();
                          final file = (sp['file'] as String?) ?? '';
                          return Padding(
                            padding: const EdgeInsets.only(top: 10),
                            child: Row(
                              children: [
                                Expanded(
                                  child: Text(
                                    '正在发送：${file.isEmpty ? '文件' : file}（${sp['index']}/${sp['total']}）',
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: TextStyle(fontSize: 11.5, color: kMut),
                                  ),
                                ),
                                NeoButton('取消发送', small: true, danger: true,
                                    onTap: () => _svc.cancelCurrentSend()),
                              ],
                            ),
                          );
                        },
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  Widget _stype(IconData icon, String label, VoidCallback onTap) {
    return Expanded(
      child: GestureDetector(
        onTap: onTap,
        child: Container(
          padding: const EdgeInsets.symmetric(vertical: 12),
          decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(13), boxShadow: kUpSm),
          child: Column(
            children: [
              Icon(icon, size: 20, color: kAcc),
              const SizedBox(height: 6),
              Text(label, style: TextStyle(fontSize: 11.5, fontWeight: FontWeight.w600, color: kFg)),
            ],
          ),
        ),
      ),
    );
  }

  Widget _pendingRow(int i) {
    final it = _pending[i];
    final isText = it.isText;
    return Container(
      constraints: const BoxConstraints(minHeight: 44),
      padding: const EdgeInsets.symmetric(vertical: 6),
      decoration: i == 0
          ? null
          : BoxDecoration(
              border: Border(top: BorderSide(color: kLt, width: 1)),
            ),
      child: Row(
        children: [
          Container(
            width: 30,
            height: 30,
            decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(9), boxShadow: kUpSm),
            child: Icon(isText ? Icons.notes_outlined : Icons.insert_drive_file_outlined, size: 15, color: kAcc),
          ),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(it.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: kFg)),
                Text(isText ? '文本' : fmtBytes(it.size), style: TextStyle(fontSize: 11, color: kMut)),
              ],
            ),
          ),
          GestureDetector(
            onTap: () => _removeItem(i),
            child: Container(
              width: 26,
              height: 26,
              decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(9), boxShadow: kUpSm),
              child: Icon(Icons.close, size: 11, color: kMut),
            ),
          ),
        ],
      ),
    );
  }

  Widget _deviceRow(Device d) {
    final on = _checked.contains(d.fingerprint);
    final online = d.download || d.deviceType == 'mobile';
    return GestureDetector(
      onTap: () => _toggleDevice(d.fingerprint),
      behavior: HitTestBehavior.opaque,
      child: Container(
        constraints: const BoxConstraints(minHeight: 52),
        padding: const EdgeInsets.symmetric(vertical: 8),
        child: Row(
          children: [
            NeoAvatar(d.alias, hue: d.fingerprint.hashCode.abs() % 360),
            const SizedBox(width: 10),
            Expanded(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(d.alias, style: TextStyle(fontSize: 14, fontWeight: FontWeight.w600, color: kFg)),
                  Text('${d.deviceType == 'desktop' ? '电脑' : '手机'} · v${d.version}', style: TextStyle(fontSize: 11.5, color: kMut)),
                ],
              ),
            ),
            // 在线/离线 badge（对齐原型）
            Container(
              padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 3),
              decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(999), boxShadow: kInSm),
              child: Text(online ? '在线' : '离线',
                  style: TextStyle(fontSize: 11, fontWeight: FontWeight.w600, color: online ? kOk : kErr)),
            ),
            const SizedBox(width: 10),
            // 收藏星标（点星标收藏/取消收藏，不勾选设备）
            GestureDetector(
              onTap: () => _toggleFav(d.fingerprint),
              behavior: HitTestBehavior.opaque,
              child: Icon(
                _svc.isFav(d.fingerprint) ? Icons.star_rounded : Icons.star_border_rounded,
                size: 22,
                color: _svc.isFav(d.fingerprint) ? kWarn : kMut.withValues(alpha: .75),
              ),
            ),
            const SizedBox(width: 12),
            // 勾选圆点
            AnimatedContainer(
              duration: const Duration(milliseconds: 180),
              width: 22,
              height: 22,
              decoration: BoxDecoration(
                color: kBg,
                shape: BoxShape.circle,
                boxShadow: on ? kInSm : kUpSm,
              ),
              alignment: Alignment.center,
              child: on
                  ? Container(width: 9, height: 9, decoration: BoxDecoration(color: kAcc, shape: BoxShape.circle))
                  : null,
            ),
          ],
        ),
      ),
    );
  }

  Widget _tool(IconData icon, String tip, VoidCallback onTap, {Color? color}) {
    return Tooltip(
      message: tip,
      child: NeoIconBtn(icon, size: 32, radius: 10, color: color ?? kMut, onTap: onTap),
    );
  }
}