// 接收页：接收就绪动画卡（对齐 PC 接收页，别名 + 自动保存开关）
//       + 接收中进度卡（activeSessions 实时进度）+ 请求确认块（pending，有请求时显示搜索框）
// 接收记录已归入顶栏历史入口，此页不再重复展示
import 'package:collection/collection.dart';
import 'package:flutter/material.dart';
import 'package:shared_preferences/shared_preferences.dart';

import '../models.dart';
import '../net/localsend.dart';
import '../theme.dart';
import '../widgets/neumorph.dart';

class RecvPage extends StatefulWidget {
  const RecvPage({super.key});

  @override
  State<RecvPage> createState() => _RecvPageState();
}

class _RecvPageState extends State<RecvPage> with SingleTickerProviderStateMixin {
  late final AnimationController _rdar =
      AnimationController(vsync: this, duration: const Duration(milliseconds: 2400))..repeat();
  final _searchCtrl = TextEditingController();
  String _q = '';

  @override
  void dispose() {
    _rdar.dispose();
    _searchCtrl.dispose();
    super.dispose();
  }

  Future<void> _setAutoSave(bool v) async {
    setState(() => LocalSendService.instance.autoAccept = v);
    try {
      final prefs = await SharedPreferences.getInstance();
      await prefs.setBool('autoAccept', v);
    } catch (_) {/* 写盘失败不影响本次会话 */}
  }

  @override
  Widget build(BuildContext context) {
    final svc = LocalSendService.instance;
    return SingleChildScrollView(
      padding: const EdgeInsets.fromLTRB(16, 14, 16, 104),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          // pending / activeSessions 任一变化都刷新
          ListenableBuilder(
            listenable: Listenable.merge([svc.pending, svc.activeSessions]),
            builder: (_, __) {
              // ── 接收中：进度卡（置顶）+ 新请求仍可确认 ──
              final actives = svc.activeSessions.value;
              if (actives.isNotEmpty) {
                return Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    for (final s in actives) _ProgressCard(session: s),
                    if (svc.pending.value.isNotEmpty) ...[
                      const SizedBox(height: 4),
                      NeoSectionLabel('接收请求（${svc.pending.value.length}）'),
                      for (final s in svc.pending.value) _RequestCard(session: s),
                    ],
                  ],
                );
              }
              // ── 无接收中：无请求 → 就绪动画卡；有请求 → 搜索框 + 确认列表 ──
              final pending = svc.pending.value;
              if (pending.isEmpty) {
                return _ReadyCard(rdar: _rdar, svc: svc, onAutoSave: _setAutoSave);
              }
              final list = pending
                  .where((s) => _q.isEmpty || s.senderAlias.toLowerCase().contains(_q) || s.files.any((f) => f.fileName.toLowerCase().contains(_q)))
                  .toList();
              return Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  NeoSearchBox(
                    controller: _searchCtrl,
                    hint: '搜索发送方 / 内容…',
                    onChanged: (v) => setState(() => _q = v.trim().toLowerCase()),
                  ),
                  const SizedBox(height: 4),
                  NeoSectionLabel('接收请求（${list.length}）'),
                  for (final s in list) _RequestCard(session: s),
                ],
              );
            },
          ),
        ],
      ),
    );
  }
}

/// 接收就绪动画卡（加大雷达 + 别名 + 自动保存开关 + 说明）
class _ReadyCard extends StatelessWidget {
  final Animation<double> rdar;
  final LocalSendService svc;
  final ValueChanged<bool> onAutoSave;
  const _ReadyCard({required this.rdar, required this.svc, required this.onAutoSave});

  @override
  Widget build(BuildContext context) {
    return NeoCard(
      Column(
        crossAxisAlignment: CrossAxisAlignment.center,
        children: [
          const SizedBox(height: 6),
          _RdarAnimation(anim: rdar),
          const SizedBox(height: 18),
          // 别名随设置联动刷新
          ListenableBuilder(
            listenable: svc,
            builder: (_, __) => Text(
              svc.alias,
              style: const TextStyle(fontSize: 17, fontWeight: FontWeight.w700, color: kFg),
            ),
          ),
          const SizedBox(height: 16),
          Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              const Text('自动保存', style: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: kFg)),
              const SizedBox(width: 8),
              NeoSwitch(value: svc.autoAccept, onChanged: onAutoSave),
            ],
          ),
          const SizedBox(height: 16),
          const Text(
            '局域网内的设备可以向本机推送文件，接收记录可在顶栏「历史」中查看。',
            textAlign: TextAlign.center,
            style: TextStyle(fontSize: 12.5, color: kMut, height: 1.7),
          ),
          const SizedBox(height: 8),
        ],
      ),
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 26),
      margin: const EdgeInsets.only(bottom: 18),
    );
  }
}

/// 雷达脉冲动画（168px 内凹圆 + 双层扩散圈 + 中心下载图标，对齐 PC .rdar）
class _RdarAnimation extends StatelessWidget {
  final Animation<double> anim;
  const _RdarAnimation({required this.anim});

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: anim,
      builder: (_, __) {
        final t = anim.value; // 0..1 每周期
        return SizedBox(
          width: 168,
          height: 168,
          child: Stack(
            alignment: Alignment.center,
            children: [
              // 内凹圆底
              Container(width: 168, height: 168, decoration: BoxDecoration(shape: BoxShape.circle, color: kBg, boxShadow: kIn)),
              // 两层扩散圈（间隔 0.5 周期）
              for (final d in [t, (t + 0.5) % 1.0])
                Transform.scale(
                  scale: 0.55 + d * 0.45,
                  child: Opacity(
                    opacity: (1 - d) * 0.9,
                    child: Container(width: 168, height: 168, decoration: BoxDecoration(shape: BoxShape.circle, border: Border.all(color: kAcc, width: 2.5))),
                  ),
                ),
              const Icon(Icons.file_download_outlined, color: kAcc, size: 54),
            ],
          ),
        );
      },
    );
  }
}

/// 接收中进度卡（当前文件 + 新拟物进度条 + 已收/总字节）
class _ProgressCard extends StatelessWidget {
  final IncomingSession session;
  const _ProgressCard({required this.session});

  @override
  Widget build(BuildContext context) {
    final files = session.files;
    final total = session.totalSize;
    final done = files.fold<int>(0, (a, f) => a + f.written);
    final pct = total <= 0 ? 0.0 : (done / total).clamp(0.0, 1.0);
    final cur = files.where((f) => f.state == 'InProgress').firstOrNull ?? files.first;
    final finished = files.where((f) => f.state == 'Finished').length;
    return NeoCard(
      Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              const Icon(Icons.download_rounded, size: 17, color: kAcc),
              const SizedBox(width: 9),
              const Expanded(child: Text('接收中', style: TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600, color: kFg))),
              Text('${(pct * 100).toStringAsFixed(0)}%', style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w700, color: kAcc)),
            ],
          ),
          const SizedBox(height: 12),
          Text('当前文件：${cur.fileName}', maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: kFg)),
          const SizedBox(height: 9),
          // 新拟物进度条（内凹轨道 + 凸起填充）
          Container(
            height: 10,
            decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(999), boxShadow: kInSm),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(999),
              child: FractionallySizedBox(
                widthFactor: pct,
                child: Container(
                  decoration: BoxDecoration(
                    color: kAcc,
                    boxShadow: [BoxShadow(color: kAcc.withValues(alpha: 0.35), blurRadius: 2, spreadRadius: 1)],
                  ),
                ),
              ),
            ),
          ),
          const SizedBox(height: 8),
          Row(
            children: [
              Expanded(
                child: Text(
                  '${fmtBytes(done)} / ${fmtBytes(total)}',
                  style: const TextStyle(fontSize: 11.5, color: kMut),
                ),
              ),
              if (files.length > 1)
                Text('文件 $finished/${files.length}', style: const TextStyle(fontSize: 11.5, color: kMut)),
            ],
          ),
        ],
      ),
      padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 14),
      margin: const EdgeInsets.only(bottom: 18),
    );
  }
}

/// 接收确认请求卡
class _RequestCard extends StatelessWidget {
  final IncomingSession session;
  const _RequestCard({required this.session});

  @override
  Widget build(BuildContext context) {
    final svc = LocalSendService.instance;
    final files = session.files;
    return NeoCard(
      Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              NeoAvatar(session.senderAlias, size: 42, radius: 14, hue: session.senderAlias.hashCode.abs() % 360),
              const SizedBox(width: 10),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(session.senderAlias, style: const TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600, color: kFg)),
                    Text('IP ${session.senderIp} · 请求接收 ${files.length} 个内容', style: const TextStyle(fontSize: 11.5, color: kMut)),
                  ],
                ),
              ),
            ],
          ),
          const SizedBox(height: 10),
          for (final f in files)
            Padding(
              padding: const EdgeInsets.symmetric(vertical: 3),
              child: Row(
                children: [
                  Icon(f.isText ? Icons.notes_outlined : Icons.insert_drive_file_outlined, size: 15, color: kAcc),
                  const SizedBox(width: 8),
                  Expanded(child: Text(f.fileName, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: kFg))),
                  Text(f.isText ? '文本' : fmtBytes(f.size), style: const TextStyle(fontSize: 11.5, color: kMut)),
                ],
              ),
            ),
          const SizedBox(height: 12),
          Row(
            children: [
              Expanded(
                child: NeoButton(
                  '拒绝',
                  danger: true,
                  onTap: () => svc.rejectSession(session.sessionId),
                ),
              ),
              const SizedBox(width: 10),
              Expanded(
                child: NeoButton(
                  '接收',
                  primary: true,
                  onTap: () {
                    svc.acceptSession(session.sessionId);
                    Toast.show(context, '已开始接收');
                  },
                ),
              ),
            ],
          ),
        ],
      ),
    );
  }
}