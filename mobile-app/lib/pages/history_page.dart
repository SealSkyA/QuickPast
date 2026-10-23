// 历史记录页：返回 + 打开目录 + 删除历史 + 搜索 + 收发记录列表
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

import '../history_store.dart';
import '../models.dart';
import '../net/localsend.dart';
import '../open_dir.dart';
import '../theme.dart';
import '../widgets/neumorph.dart';

class HistoryPage extends StatefulWidget {
  const HistoryPage({super.key});

  @override
  State<HistoryPage> createState() => _HistoryPageState();
}

class _HistoryPageState extends State<HistoryPage> {
  String _q = '';

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: kBg,
      body: SafeArea(
        child: Column(
          children: [
            // 顶栏
            Padding(
              padding: const EdgeInsets.fromLTRB(18, 6, 18, 12),
              child: Row(
                children: [
                  NeoIconBtn(Icons.arrow_back_ios_new_rounded, size: 38, radius: 13, color: kFg, onTap: () => Navigator.pop(context)),
                  Expanded(
                    child: Padding(
                      padding: EdgeInsets.only(left: 10),
                      child: Text('历史记录', style: TextStyle(fontSize: 19, fontWeight: FontWeight.w600, color: kFg)),
                    ),
                  ),
                  _openDirBtn(),
                  const SizedBox(width: 8),
                  NeoIconBtn(Icons.delete_outline_rounded, size: 38, radius: 13, color: kErr, onTap: _clear),
                ],
              ),
            ),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16),
              child: NeoSearchBox(onChanged: (v) => setState(() => _q = v.trim().toLowerCase()), hint: '搜索记录…'),
            ),
            Expanded(
              child: ListenableBuilder(
                listenable: HistoryStore.instance,
                builder: (_, __) {
                  final all = HistoryStore.instance.entries;
                  final list = _q.isEmpty ? all : all.where((e) => e.peerAlias.toLowerCase().contains(_q) || e.summary.toLowerCase().contains(_q)).toList();
                  if (list.isEmpty) {
                    return Center(
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          Icon(Icons.history_rounded, size: 40, color: kMut.withValues(alpha: .6)),
                          const SizedBox(height: 10),
                          Text('暂无收发记录', style: TextStyle(fontSize: 13, color: kMut)),
                        ],
                      ),
                    );
                  }
                  return ListView(
                    padding: const EdgeInsets.fromLTRB(16, 6, 16, 30),
                    children: [
                      for (var i = 0; i < list.length; i++) ...[
                        if (i > 0) const SizedBox(height: 10),
                        _entry(list[i]),
                      ],
                    ],
                  );
                },
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _openDirBtn() {
    return GestureDetector(
      onTap: _openDir,
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 13, vertical: 8),
        decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(12), boxShadow: kUpSm),
        child: Row(
          children: [
            Icon(Icons.folder_open_outlined, size: 16, color: kAcc),
            const SizedBox(width: 6),
            Text('打开目录', style: TextStyle(fontSize: 12.5, fontWeight: FontWeight.w600, color: kAcc.withValues(alpha: 0.95))),
          ],
        ),
      ),
    );
  }

  Future<void> _openDir() async {
    final svc = LocalSendService.instance;
    var dir = svc.saveDir;
    if (dir.isEmpty || !Directory(dir).existsSync()) {
      // 默认外部目录
      final ext = await getExternalStorageDirectory();
      final candidate = Directory('/storage/emulated/0/Download/QuickCopy');
      if (await candidate.exists()) {
        dir = candidate.path;
      } else {
        dir = ext?.path ?? '';
      }
    }
    if (dir.isEmpty || !Directory(dir).existsSync()) {
      Toast.show(context, '尚未设置保存目录');
      return;
    }
    final ok = await openDirInFiles(dir);
    if (!ok && mounted) Toast.show(context, '打开目录失败\n可在文件管理器访问：\n$dir');
  }

  void _clear() {
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
              Text('删除全部历史记录？', style: TextStyle(fontSize: 15, fontWeight: FontWeight.w600, color: kFg)),
              const SizedBox(height: 14),
              Row(
                children: [
                  Expanded(child: NeoButton('取消', onTap: () => Navigator.pop(ctx))),
                  const SizedBox(width: 10),
                  Expanded(
                    child: NeoButton('删除', danger: true, onTap: () {
                      HistoryStore.instance.clear();
                      Navigator.pop(ctx);
                      Toast.show(context, '历史记录已清空');
                    }),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
  }

  Widget _entry(HistoryEntry e) {
    final txt = e.incoming ? '接收' : '发送';
    return NeoCard(
      Row(
        children: [
          NeoAvatar(e.peerAlias, hue: e.peerAlias.hashCode.abs() % 360),
          const SizedBox(width: 10),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text(e.summary, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 13.5, fontWeight: FontWeight.w600, color: kFg)),
                Text('$txt · ${e.peerAlias} · ${e.detail}', maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 11.5, color: kMut)),
              ],
            ),
          ),
          Text(_fmt(e.time), style: TextStyle(fontSize: 11.5, color: kMut)),
        ],
      ),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
      margin: EdgeInsets.zero,
    );
  }

  static String _fmt(DateTime t) {
    final hm = '${t.hour.toString().padLeft(2, '0')}:${t.minute.toString().padLeft(2, '0')}';
    final now = DateTime.now();
    if (t.year == now.year && t.month == now.month && t.day == now.day) return hm;
    return '${t.month}/${t.day} $hm';
  }
}