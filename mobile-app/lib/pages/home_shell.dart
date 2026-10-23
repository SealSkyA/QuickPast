// 首页外壳：顶栏（在线 pill + 历史入口）+ 三 Tab（接收/发送/我的）
import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../history_store.dart';
import '../models.dart';
import '../net/localsend.dart';
import '../theme.dart';
import '../widgets/neumorph.dart';
import 'history_page.dart';
import 'me_page.dart';
import 'recv_page.dart';
import 'send_page.dart';

class HomeShell extends StatefulWidget {
  const HomeShell({super.key});

  @override
  State<HomeShell> createState() => _HomeShellState();
}

class _HomeShellState extends State<HomeShell> {
  int _tab = 0;
  bool _pillDown = false;
  final _svc = LocalSendService.instance;
  final ValueNotifier<int> _pendingCount = ValueNotifier(0);

  @override
  void initState() {
    super.initState();
    _svc.pending.addListener(() {
      _pendingCount.value = _svc.pending.value.length;
    });
    // 收发完成 → Toast + 历史
    _svc.rcvDone.stream.listen((e) {
      final files = (e['files'] as List).cast<IncomingFile>();
      final total = files.fold<int>(0, (a, f) => a + f.size);
      final names = files.map((f) => f.fileName).take(3).join('、');
      final ok = files.every((f) => f.state == 'Finished');
      HistoryStore.instance.add(HistoryEntry(
        incoming: true,
        peerAlias: (e['senderAlias'] as String?) ?? '?',
        summary: names.isEmpty ? '（无文件）' : names,
        detail: '${fmtBytes(total)}',
        time: DateTime.now(),
        ok: ok,
      ));
      if (mounted) {
        Toast.show(context, ok ? '已接收来自「${e['senderAlias']}」的内容' : '部分接收失败');
      }
    });
    _svc.sendDone.stream.listen((e) {
      final ok = e['ok'] == true;
      HistoryStore.instance.add(HistoryEntry(
        incoming: false,
        peerAlias: (e['device'] as String?) ?? '?',
        summary: '${e['items'] ?? 0} 个文件',
        detail: ok ? '已发送' : '发送失败',
        time: DateTime.now(),
        ok: ok,
      ));
      if (mounted) {
        Toast.show(context, ok ? '已发送给「${e['device']}」' : '发送失败：${e['error']}');
      }
    });
  }

  void _toggleOnline() async {
    setState(() {
      _svc.online = !_svc.online;
    });
    if (_svc.online) _svc.refresh();
    final prefs = await SharedPreferences.getInstance();
    await prefs.setBool('online', _svc.online);
    if (mounted) {
      Toast.show(context, _svc.online ? '已在线，局域网可发现本机' : '已离线，局域网不广播本机');
    }
  }

  void _openHistory() {
    Navigator.of(context).push(MaterialPageRoute(builder: (_) => const HistoryPage()));
  }

  @override
  Widget build(BuildContext context) {
    // 监听主题模式：切换时重建整页（IndexedStack 子页同类型会保留 State，仅刷新颜色）
    return ValueListenableBuilder<ThemeMode>(
      valueListenable: appThemeMode,
      builder: (_, mode, __) => Scaffold(
        backgroundColor: kBg,
        body: SafeArea(
          child: Column(
            children: [
              // 顶栏
              Padding(
                padding: const EdgeInsets.fromLTRB(18, 6, 18, 4),
                child: Row(
                  children: [
                    Expanded(
                      child: Text('QuickCopy', style: TextStyle(fontSize: 20, fontWeight: FontWeight.w600, letterSpacing: .3, color: kFg)),
                    ),
                    GestureDetector(
                      onTapDown: (_) => setState(() => _pillDown = true),
                      onTapCancel: () => setState(() => _pillDown = false),
                      onTapUp: (_) => setState(() => _pillDown = false),
                      onTap: _toggleOnline,
                      child: _onlinePill(),
                    ),
                    const SizedBox(width: 10),
                    _historyButton(),
                  ],
                ),
              ),
              // 内容区
              Expanded(
                child: IndexedStack(
                  index: _tab,
                  children: [RecvPage(), SendPage(), MePage()],
                ),
              ),
            ],
          ),
        ),
        bottomNavigationBar: Container(
          height: 70,
          decoration: BoxDecoration(color: kBg),
          child: Row(
            children: [
              _tabItem(0, Icons.file_download_outlined, '接收'),
              _tabItem(1, Icons.send_outlined, '发送'),
              _tabItem(2, Icons.person_outline, '我的'),
            ],
          ),
        ),
      ),
    );
  }

  Widget _onlinePill() {
    return AnimatedContainer(
      duration: const Duration(milliseconds: 160),
      height: 30,
      padding: const EdgeInsets.symmetric(horizontal: 12),
      decoration: BoxDecoration(
        color: kBg,
        borderRadius: BorderRadius.circular(999),
        boxShadow: _pillDown ? kIn : kInSm,
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Container(
            width: 7,
            height: 7,
            decoration: BoxDecoration(
              shape: BoxShape.circle,
              color: _svc.online ? kOk : kErr,
              boxShadow: [BoxShadow(color: (_svc.online ? kOk : kErr).withValues(alpha: .25), spreadRadius: 3)],
            ),
          ),
          const SizedBox(width: 6),
          Text(
            _svc.online ? '在线' : '离线',
            style: TextStyle(fontSize: 11.5, fontWeight: FontWeight.w600, color: _svc.online ? kOk : kErr),
          ),
        ],
      ),
    );
  }

  Widget _historyButton() {
    return ValueListenableBuilder<int>(
      valueListenable: _pendingCount,
      builder: (_, count, __) {
        return Stack(
          clipBehavior: Clip.none,
          children: [
            NeoIconBtn(Icons.history_rounded, size: 38, radius: 13, color: kAcc, onTap: _openHistory),
            if (count > 0)
              Positioned(
                right: -4,
                top: -4,
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 5, vertical: 1),
                  decoration: BoxDecoration(color: kErr, borderRadius: BorderRadius.circular(999), boxShadow: kUpSm),
                  child: Text('$count', style: const TextStyle(fontSize: 10, color: Colors.white, fontWeight: FontWeight.w600)),
                ),
              ),
          ],
        );
      },
    );
  }

  Widget _tabItem(int i, IconData icon, String label) {
    final on = _tab == i;
    return Expanded(
      child: GestureDetector(
        onTap: () => setState(() => _tab = i),
        behavior: HitTestBehavior.opaque,
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            AnimatedContainer(
              duration: const Duration(milliseconds: 180),
              width: 44,
              height: 28,
              decoration: BoxDecoration(
                color: kBg,
                borderRadius: BorderRadius.circular(999),
                boxShadow: on ? kInSm : const [],
              ),
              child: Icon(icon, size: 20, color: on ? kAcc : kMut),
            ),
            const SizedBox(height: 3),
            Text(label, style: TextStyle(fontSize: 10.5, fontWeight: FontWeight.w600, color: on ? kAcc : kMut)),
          ],
        ),
      ),
    );
  }
}