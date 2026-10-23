// 新拟物 Neumorphism 主题令牌（对齐 PC 端 UI-风格规范-新拟物.md）
// 亮/暗双模式：组件库直接引用全局 token（kBg/kFg/阴影等），
// 切换主题时调用 applyThemeMode() 重赋值并广播，HomeShell 监听后重建整页。
import 'package:flutter/material.dart';

/// 应用当前主题模式（main 的 MaterialApp.themeMode 与全局 token 联动）
final ValueNotifier<ThemeMode> appThemeMode = ValueNotifier(ThemeMode.system);

// ────────── 当前生效 token（组件库直接引用，切换主题时整体重赋值）──────────
Color kBg = const Color(0xFFE9EDF2);
Color kLt = const Color(0xFFFFFFFF);
Color kSh = const Color(0xFFC0C7D2);
Color kLine = const Color(0xFFCFD6E0);
Color kFg = const Color(0xFF3C4453);
Color kMut = const Color(0xFF7F8A9B);
Color kAcc = const Color(0xFF4D6FA4);
Color kOk = const Color(0xFF2F8F6D);
Color kWarn = const Color(0xFFA5762C);
Color kErr = const Color(0xFFB3524D);

const double kR = 16;        // 卡片圆角
const double kRR = 12;       // 组件圆角
const double kRadiusCapsule = 999;

/// 当前应用版本（升级时与 pubspec.yaml 的 version 同步维护）
const String kAppVersion = '0.2.16';

// 双层阴影（凸起）——按当前 token 实时重算，亮/暗主题自动适配
List<BoxShadow> get kUp => [
  BoxShadow(color: kLt, offset: const Offset(-5, -5), blurRadius: 11),
  BoxShadow(color: kSh, offset: const Offset(5, 5), blurRadius: 12),
];
List<BoxShadow> get kUpSm => [
  BoxShadow(color: kLt, offset: const Offset(-3, -3), blurRadius: 7),
  BoxShadow(color: kSh, offset: const Offset(3, 3), blurRadius: 8),
];
// 凹陷（Flutter BoxShadow 无 inset；用负 spreadRadius 让阴影收缩进组件
// 边缘内部——内边缘柔和过渡、中心纯 kBg、外部无灰晕，真机与原型一致）
List<BoxShadow> get kIn => [
  BoxShadow(color: kLt.withValues(alpha: 0.8), offset: const Offset(-3, -3), blurRadius: 7, spreadRadius: -2),
  BoxShadow(color: kSh.withValues(alpha: 0.8), offset: const Offset(3, 3), blurRadius: 8, spreadRadius: -2),
];
List<BoxShadow> get kInSm => [
  BoxShadow(color: kLt.withValues(alpha: 0.8), offset: const Offset(-2, -2), blurRadius: 5, spreadRadius: -2),
  BoxShadow(color: kSh.withValues(alpha: 0.8), offset: const Offset(2, 2), blurRadius: 6, spreadRadius: -2),
];

const String kFontStack = 'PingFang SC, Microsoft YaHei, Segoe UI, sans-serif';

/// 是否实际暗色（跟随系统按当前系统亮度）
bool effectiveDark(ThemeMode mode) {
  if (mode == ThemeMode.dark) return true;
  if (mode == ThemeMode.light) return false;
  return WidgetsBinding.instance.platformDispatcher.platformBrightness == Brightness.dark;
}

/// 切换主题：重赋全局 token 并广播（HomeShell 监听重建页面，MaterialApp.themeMode 同步）
void applyThemeMode(ThemeMode mode) {
  if (effectiveDark(mode)) {
    kBg = const Color(0xFF17181C);
    kLt = const Color(0xFF26282E);   // 暗色高光（比底色亮一档）
    kSh = const Color(0xFF0B0C0F);   // 暗色阴影
    kLine = const Color(0xFF2E3138);
    kFg = const Color(0xFFD9DEE7);
    kMut = const Color(0xFF8B94A5);
    kAcc = const Color(0xFF7D9CD3);
    kOk = const Color(0xFF46B08C);
    kWarn = const Color(0xFFC99B4D);
    kErr = const Color(0xFFD06B65);
  } else {
    kBg = const Color(0xFFE9EDF2);
    kLt = const Color(0xFFFFFFFF);
    kSh = const Color(0xFFC0C7D2);
    kLine = const Color(0xFFCFD6E0);
    kFg = const Color(0xFF3C4453);
    kMut = const Color(0xFF7F8A9B);
    kAcc = const Color(0xFF4D6FA4);
    kOk = const Color(0xFF2F8F6D);
    kWarn = const Color(0xFFA5762C);
    kErr = const Color(0xFFB3524D);
  }
  appThemeMode.value = mode;
}

ThemeData buildNeumorphTheme() {
  final base = ThemeData(
    useMaterial3: false,
    fontFamily: kFontStack,
    scaffoldBackgroundColor: kBg,
    colorScheme: ColorScheme.light(
      primary: kAcc,
      secondary: kOk,
      surface: kBg,
      error: kErr,
      onSurface: kFg,
    ),
    splashFactory: NoSplash.splashFactory,
    highlightColor: Colors.transparent,
  );
  return base.copyWith(
    textTheme: base.textTheme
        .apply(bodyColor: kFg, displayColor: kFg)
        .copyWith(
          titleLarge: TextStyle(fontSize: 20, fontWeight: FontWeight.w600, letterSpacing: .3, color: kFg),
          titleMedium: TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600, letterSpacing: .2, color: kFg),
          bodyMedium: TextStyle(fontSize: 14, color: kFg),
          bodySmall: TextStyle(fontSize: 11.5, color: kMut),
        ),
    platform: TargetPlatform.android,
  );
}

ThemeData buildNeumorphDarkTheme() {
  final base = ThemeData(
    useMaterial3: false,
    fontFamily: kFontStack,
    scaffoldBackgroundColor: kBg,
    brightness: Brightness.dark,
    colorScheme: ColorScheme.dark(
      primary: kAcc,
      secondary: kOk,
      surface: kBg,
      error: kErr,
      onSurface: kFg,
    ),
    splashFactory: NoSplash.splashFactory,
    highlightColor: Colors.transparent,
  );
  return base.copyWith(
    textTheme: base.textTheme
        .apply(bodyColor: kFg, displayColor: kFg)
        .copyWith(
          titleLarge: TextStyle(fontSize: 20, fontWeight: FontWeight.w600, letterSpacing: .3, color: kFg),
          titleMedium: TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600, letterSpacing: .2, color: kFg),
          bodyMedium: TextStyle(fontSize: 14, color: kFg),
          bodySmall: TextStyle(fontSize: 11.5, color: kMut),
        ),
    platform: TargetPlatform.android,
  );
}
