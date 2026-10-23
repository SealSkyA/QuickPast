// 新拟物组件库（凸起卡片/凹凸按钮/开关/胶囊/刻痕行/输入框/Toast）
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../theme.dart';

/// 凸起卡片
class NeoCard extends StatelessWidget {
  final Widget child;
  final EdgeInsetsGeometry? padding;
  final double radius;
  final EdgeInsetsGeometry? margin;
  const NeoCard(this.child, {super.key, this.padding, this.radius = kR, this.margin});

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: margin ?? const EdgeInsets.only(bottom: 18),
      padding: padding ?? const EdgeInsets.all(16),
      decoration: BoxDecoration(
        color: kBg,
        borderRadius: BorderRadius.circular(radius),
        boxShadow: kUp,
      ),
      child: child,
    );
  }
}

/// 卡片标题行（图标 + h2）
class NeoCardHeader extends StatelessWidget {
  final IconData icon;
  final String title;
  const NeoCardHeader(this.icon, this.title, {super.key});

  @override
  Widget build(BuildContext context) {
    return Row(
      children: [
        Icon(icon, size: 17, color: kAcc),
        const SizedBox(width: 9),
        Text(title, style: Theme.of(context).textTheme.titleMedium),
      ],
    );
  }
}

/// 刻痕分隔行（白线压灰线 + 固定最小高度）
class NeoRow extends StatelessWidget {
  final Widget? left;
  final Widget? right;
  final bool isFirst;
  final EdgeInsetsGeometry? padding;
  const NeoRow({super.key, this.left, this.right, this.isFirst = false, this.padding});

  @override
  Widget build(BuildContext context) {
    return Container(
      constraints: const BoxConstraints(minHeight: 52),
      padding: padding ?? const EdgeInsets.symmetric(vertical: 8),
      decoration: isFirst
          ? null
          : BoxDecoration(
              border: Border(top: BorderSide(color: kLt, width: 1)),
              // 灰线去除：行底背景保持与首行一致（用户反馈灰线观感突兀）
              // boxShadow: [BoxShadow(color: Color(0x4DBFC6D0), offset: Offset(0, 1), blurRadius: 0)],
            ),
      child: Row(
        children: [
          if (left != null) Expanded(child: left!),
          if (right != null) right!,
        ],
      ),
    );
  }
}

/// 行标题（可带副标题说明）
class NeoRowTitle extends StatelessWidget {
  final String title;
  final String? sub;
  const NeoRowTitle(this.title, {super.key, this.sub});

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Text(title, style: Theme.of(context).textTheme.bodyMedium?.copyWith(fontWeight: FontWeight.w600)),
        if (sub != null)
          Padding(
            padding: const EdgeInsets.only(top: 1),
            child: Text(sub!, style: Theme.of(context).textTheme.bodySmall),
          ),
      ],
    );
  }
}

/// 按钮（按压凸变凹）
class NeoButton extends StatefulWidget {
  final String text;
  final VoidCallback? onTap;
  final bool primary;
  final bool danger;
  final bool small;
  final bool enabled;
  const NeoButton(
    this.text, {
    super.key,
    this.onTap,
    this.primary = false,
    this.danger = false,
    this.small = false,
    this.enabled = true,
  });

  @override
  State<NeoButton> createState() => _NeoButtonState();
}

class _NeoButtonState extends State<NeoButton> {
  bool _down = false;

  @override
  Widget build(BuildContext context) {
    final enabled = widget.enabled && widget.onTap != null;
    final bg = widget.primary ? kAcc : kBg;
    final fg = widget.primary ? const Color(0xFFF2F5F9) : (widget.danger ? kErr : kFg);
    return GestureDetector(
      onTapDown: (_) => setState(() => _down = true),
      onTapCancel: () => setState(() => _down = false),
      onTapUp: (_) => setState(() => _down = false),
      onTap: enabled ? widget.onTap : null,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        width: widget.small ? null : double.infinity,
        padding: EdgeInsets.symmetric(horizontal: widget.small ? 14 : 16, vertical: widget.small ? 7 : 12),
        decoration: BoxDecoration(
          color: bg,
          borderRadius: BorderRadius.circular(13),
          boxShadow: !enabled
              ? const []
              : _down
                  ? kInSm
                  : kUpSm,
        ),
        child: Text(
          widget.text,
          textAlign: TextAlign.center,
          style: TextStyle(
            fontSize: widget.small ? 12.5 : 14,
            fontWeight: FontWeight.w600,
            color: enabled ? fg : kMut.withValues(alpha: 0.5),
          ),
        ),
      ),
    );
  }
}

/// 小图标按钮（38px 顶栏 / 工具钮）
class NeoIconBtn extends StatefulWidget {
  final IconData icon;
  final double size;
  final double radius;
  final Color? color;
  final VoidCallback? onTap;
  const NeoIconBtn(this.icon, {super.key, this.size = 38, this.radius = 13, this.color = const Color(0xFF4D6FA4), this.onTap});

  @override
  State<NeoIconBtn> createState() => _NeoIconBtnState();
}

class _NeoIconBtnState extends State<NeoIconBtn> {
  bool _down = false;

  @override
  Widget build(BuildContext context) {
    final hasTap = widget.onTap != null;
    return GestureDetector(
      // onTap 为空时注册任何 tap 回调会吞掉外层手势，故仅在有 onTap 时注册
      onTapDown: hasTap ? (_) => setState(() => _down = true) : null,
      onTapCancel: hasTap ? () => setState(() => _down = false) : null,
      onTapUp: hasTap ? (_) => setState(() => _down = false) : null,
      onTap: widget.onTap,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 150),
        width: widget.size,
        height: widget.size,
        decoration: BoxDecoration(
          color: kBg,
          borderRadius: BorderRadius.circular(widget.radius),
          boxShadow: _down ? kInSm : kUpSm,
        ),
        child: Icon(widget.icon, size: widget.size * 0.5, color: widget.color),
      ),
    );
  }
}

/// 凹陷胶囊（在线/离线 pill、badge）
class NeoPill extends StatelessWidget {
  final String text;
  final Color color;
  final VoidCallback? onTap;
  const NeoPill(this.text, {super.key, this.color = const Color(0xFF2F8F6D), this.onTap});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: onTap,
      child: Container(
        height: 30,
        padding: const EdgeInsets.symmetric(horizontal: 12),
        decoration: BoxDecoration(
          color: kBg,
          borderRadius: BorderRadius.circular(999),
          boxShadow: kInSm,
        ),
        child: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              width: 7,
              height: 7,
              decoration: BoxDecoration(
                color: color,
                shape: BoxShape.circle,
                boxShadow: [BoxShadow(color: color.withValues(alpha: 0.25), blurRadius: 0, spreadRadius: 3)],
              ),
            ),
            const SizedBox(width: 6),
            Text(text, style: TextStyle(fontSize: 11.5, fontWeight: FontWeight.w600, color: color)),
          ],
        ),
      ),
    );
  }
}

/// 新拟物开关
class NeoSwitch extends StatelessWidget {
  final bool value;
  final ValueChanged<bool>? onChanged;
  final bool enabled;
  const NeoSwitch({super.key, required this.value, this.onChanged, this.enabled = true});

  @override
  Widget build(BuildContext context) {
    return GestureDetector(
      onTap: enabled ? () => onChanged?.call(!value) : null,
      child: AnimatedContainer(
        duration: const Duration(milliseconds: 220),
        width: 42,
        height: 24,
        decoration: BoxDecoration(
          color: kBg,
          borderRadius: BorderRadius.circular(999),
          boxShadow: value
              ? [...kInSm, const BoxShadow(color: Color(0x38345267), blurRadius: 0, spreadRadius: 2)]
              : kInSm,
        ),
        child: AnimatedAlign(
          duration: const Duration(milliseconds: 220),
          alignment: value ? Alignment.centerRight : Alignment.centerLeft,
          child: Padding(
            padding: const EdgeInsets.all(3),
            child: Container(
              width: 18,
              height: 18,
              decoration: BoxDecoration(
                color: value ? kAcc : kBg,
                shape: BoxShape.circle,
                boxShadow: kUpSm,
              ),
            ),
          ),
        ),
      ),
    );
  }
}

/// 内凹陷输入框
class NeoField extends StatelessWidget {
  final TextEditingController? controller;
  final ValueChanged<String>? onChanged;
  final String? hint;
  final double width;
  final double paddingV;
  final TextInputType? keyboardType;
  final bool readOnly;
  const NeoField({
    super.key,
    this.controller,
    this.onChanged,
    this.hint,
    this.width = 118,
    this.paddingV = 9,
    this.keyboardType,
    this.readOnly = false,
  });

  @override
  Widget build(BuildContext context) {
    return Container(
      width: width,
      decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(11), boxShadow: kInSm),
      child: TextField(
        controller: controller,
        readOnly: readOnly,
        keyboardType: keyboardType,
        onChanged: onChanged,
        style: TextStyle(fontSize: 13.5, color: kFg),
        decoration: InputDecoration(
          isDense: true,
          contentPadding: EdgeInsets.symmetric(horizontal: 10, vertical: paddingV),
          hintText: hint,
          hintStyle: TextStyle(color: kMut),
          border: InputBorder.none,
        ),
      ),
    );
  }
}

/// 下拉（凹陷，新拟物弹出菜单——原生 DropdownButton 的 Material 菜单
/// 是白底灰框，不符合新拟物；且嵌套在卡片/滚动列表里会因绘制顺序被
/// 后续兄弟节点遮挡，故用 OverlayEntry 在屏幕顶层自绘同色阴影菜单）。
class NeoSelect extends StatefulWidget {
  final String value;
  final List<String> options;
  final ValueChanged<String>? onChanged;
  final double width;
  const NeoSelect(this.value, this.options, {super.key, this.onChanged, this.width = 118});

  @override
  State<NeoSelect> createState() => _NeoSelectState();
}

class _NeoSelectState extends State<NeoSelect> {
  final _key = GlobalKey();
  bool _open = false;
  OverlayEntry? _overlay;

  void _toggle() {
    if (widget.onChanged == null) return;
    if (_open) {
      _close();
    } else {
      _openMenu();
    }
  }

  void _openMenu() {
    setState(() => _open = true);
    final overlay = Overlay.of(context, rootOverlay: true);
    final box = _key.currentContext!.findRenderObject() as RenderBox;
    final pos = box.localToGlobal(Offset.zero);
    final h = box.size.height;
    final menuH = widget.options.length * 38.0 + 12;
    // 空间不足时向上弹出，避免菜单出屏
    final screenH = MediaQuery.of(context).size.height;
    var top = pos.dy + h + 4;
    if (top + menuH > screenH - 8) top = pos.dy - menuH - 4;
    _overlay = OverlayEntry(
      builder: (_) => Stack(
        children: [
          // 半透明遮罩拦截外部点击关闭菜单（不吞点击时需 translucent）
          Positioned.fill(
            child: GestureDetector(
              onTap: _close,
              behavior: HitTestBehavior.translucent,
              child: const SizedBox.expand(),
            ),
          ),
          Positioned(left: pos.dx, top: top, width: widget.width, child: _buildMenu()),
        ],
      ),
    );
    overlay.insert(_overlay!);
  }

  void _close() {
    _overlay?.remove();
    _overlay = null;
    if (mounted) setState(() => _open = false);
  }

  void _pick(String v) {
    _close();
    widget.onChanged?.call(v);
  }

  @override
  Widget build(BuildContext context) {
    final cur = widget.options.contains(widget.value) ? widget.value : widget.options.first;
    return GestureDetector(
      key: _key,
      onTap: _toggle,
      child: Container(
        width: widget.width,
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 9),
        decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(11), boxShadow: _open ? kIn : kInSm),
        child: Row(
          children: [
            Expanded(
              child: Text(cur, maxLines: 1, overflow: TextOverflow.ellipsis, style: TextStyle(fontSize: 13.5, color: kFg)),
            ),
            AnimatedRotation(
              turns: _open ? 0.5 : 0,
              duration: const Duration(milliseconds: 180),
              child: Icon(Icons.keyboard_arrow_down, size: 18, color: kMut),
            ),
          ],
        ),
      ),
    );
  }

  Widget _buildMenu() {
    final cur = widget.options.contains(widget.value) ? widget.value : widget.options.first;
    return Container(
      decoration: BoxDecoration(
        color: kBg,
        borderRadius: BorderRadius.circular(12),
        boxShadow: kUp,
      ),
      padding: const EdgeInsets.symmetric(vertical: 5),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          for (var i = 0; i < widget.options.length; i++)
            InkWell(
              onTap: () => _pick(widget.options[i]),
              child: Container(
                width: widget.width,
                padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 9),
                decoration: i == 0
                    ? null
                    : BoxDecoration(
                        border: Border(top: BorderSide(color: kLine, width: 1)),
                      ),
                child: Text(
                  widget.options[i],
                  style: TextStyle(
                    fontSize: 13,
                    fontWeight: widget.options[i] == cur ? FontWeight.w700 : FontWeight.w500,
                    color: widget.options[i] == cur ? kAcc : kFg,
                  ),
                ),
              ),
            ),
        ],
      ),
    );
  }
}

/// 平滑圆角容器（内凹陷组包装，如高级设置折叠）
class NeoInset extends StatelessWidget {
  final Widget child;
  final EdgeInsetsGeometry? padding;
  const NeoInset(this.child, {super.key, this.padding = const EdgeInsets.symmetric(horizontal: 12, vertical: 4)});

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(top: 12),
      padding: padding,
      decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(12), boxShadow: kInSm),
      child: child,
    );
  }
}

/// 搜索框（内凹陷）
class NeoSearchBox extends StatelessWidget {
  final TextEditingController? controller;
  final ValueChanged<String>? onChanged;
  final String hint;
  const NeoSearchBox({super.key, this.controller, this.onChanged, this.hint = '搜索设备、记录'});

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(bottom: 4),
      padding: const EdgeInsets.symmetric(horizontal: 11),
      decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(12), boxShadow: kInSm),
      child: Row(
        children: [
          Icon(Icons.search, size: 15, color: kMut),
          const SizedBox(width: 8),
          Expanded(
            child: TextField(
              controller: controller,
              onChanged: onChanged,
              decoration: InputDecoration(
                hintText: hint,
                hintStyle: TextStyle(fontSize: 13, color: kMut),
                border: InputBorder.none,
                isCollapsed: true,
                contentPadding: const EdgeInsets.symmetric(vertical: 10),
              ),
              style: TextStyle(fontSize: 13, color: kFg),
            ),
          ),
        ],
      ),
    );
  }
}

/// 分类分隔标签（历史/记录）
class NeoSectionLabel extends StatelessWidget {
  final String text;
  const NeoSectionLabel(this.text, {super.key});

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(2, 2, 0, 8),
      child: Text(text, style: TextStyle(fontSize: 12, fontWeight: FontWeight.w600, color: kMut.withValues(alpha: 0.9))),
    );
  }
}

/// 头像（设备/文件）
class NeoAvatar extends StatelessWidget {
  final String text;
  final double size;
  final double radius;
  final int hue;
  const NeoAvatar(this.text, {super.key, this.size = 38, this.radius = 13, this.hue = 220});

  @override
  Widget build(BuildContext context) {
    return Container(
      width: size,
      height: size,
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(radius),
        gradient: LinearGradient(
          begin: Alignment.topLeft,
          end: Alignment.bottomRight,
          colors: [HSLColor.fromAHSL(1, hue.toDouble(), 0.4, 0.82).toColor(), HSLColor.fromAHSL(1, hue.toDouble(), 0.3, 0.7).toColor()],
        ),
        boxShadow: kUpSm,
      ),
      alignment: Alignment.center,
      child: Text(
        text.isNotEmpty ? text.characters.first.toUpperCase() : '?',
        style: TextStyle(fontSize: size * 0.4, fontWeight: FontWeight.w700, color: kFg),
      ),
    );
  }
}

/* ────────────── Toast ────────────── */

class Toast {
  static OverlayEntry? _entry;

  static void show(BuildContext context, String msg) {
    final overlay = Overlay.of(context, rootOverlay: true);
    _entry?.remove();
    final entry = OverlayEntry(
      builder: (_) => Positioned(
        left: 0,
        right: 0,
        bottom: 96,
        child: IgnorePointer(
          child: Center(
            child: ValueListenableBuilder<double>(
              valueListenable: _toastOpacity,
              builder: (_, v, __) => AnimatedOpacity(
                opacity: v,
                duration: const Duration(milliseconds: 250),
                child: Container(
                  padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 11),
                  decoration: BoxDecoration(color: kBg, borderRadius: BorderRadius.circular(999)),
                  child: Text(msg, style: TextStyle(fontSize: 13, fontWeight: FontWeight.w600, color: kFg, decoration: TextDecoration.none)),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    overlay.insert(entry);
    _entry = entry;
    _toastOpacity.value = 1.0;
    Future.delayed(const Duration(milliseconds: 1700), () {
      _toastOpacity.value = 0.0;
      Future.delayed(const Duration(milliseconds: 300), entry.remove);
      if (_entry == entry) _entry = null;
    });
  }

  static final ValueNotifier<double> _toastOpacity = ValueNotifier(0);
}

/// 复制文本到系统剪贴板（网络层委托）
void copyToClipboard(String text) {
  Clipboard.setData(ClipboardData(text: text));
}