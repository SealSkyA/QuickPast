// 新拟物 Neumorphism 主题令牌（对齐 PC 端 UI-风格规范-新拟物.md）
import 'package:flutter/material.dart';

/// 浅色底与功能色（与 PC 端 token 完全一致）
const Color kBg = Color(0xFFE9EDF2);
const Color kLt = Color(0xFFFFFFFF);
const Color kSh = Color(0xFFC0C7D2);
const Color kLine = Color(0xFFCFD6E0);
const Color kFg = Color(0xFF3C4453);
const Color kMut = Color(0xFF7F8A9B);
const Color kAcc = Color(0xFF4D6FA4);
const Color kOk = Color(0xFF2F8F6D);
const Color kWarn = Color(0xFFA5762C);
const Color kErr = Color(0xFFB3524D);

const double kR = 16;        // 卡片圆角
const double kRR = 12;       // 组件圆角
const double kRadiusCapsule = 999;

// 双层阴影（凸起）
const List<BoxShadow> kUp = [
  BoxShadow(color: kLt, offset: Offset(-5, -5), blurRadius: 11),
  BoxShadow(color: kSh, offset: Offset(5, 5), blurRadius: 12),
];
const List<BoxShadow> kUpSm = [
  BoxShadow(color: kLt, offset: Offset(-3, -3), blurRadius: 7),
  BoxShadow(color: kSh, offset: Offset(3, 3), blurRadius: 8),
];
// 凹陷（Flutter BoxShadow 无 inset；用负 spreadRadius 让阴影收缩进组件
// 边缘内部——内边缘柔和过渡、中心纯 kBg、外部无灰晕，真机与原型一致）
const List<BoxShadow> kIn = [
  BoxShadow(color: Color(0xCCFFFFFF), offset: Offset(-3, -3), blurRadius: 7, spreadRadius: -2),
  BoxShadow(color: Color(0xCCC0C7D2), offset: Offset(3, 3), blurRadius: 8, spreadRadius: -2),
];
const List<BoxShadow> kInSm = [
  BoxShadow(color: Color(0xCCFFFFFF), offset: Offset(-2, -2), blurRadius: 5, spreadRadius: -2),
  BoxShadow(color: Color(0xCCC0C7D2), offset: Offset(2, 2), blurRadius: 6, spreadRadius: -2),
];

const String kFontStack = 'PingFang SC, Microsoft YaHei, Segoe UI, sans-serif';

ThemeData buildNeumorphTheme() {
  final base = ThemeData(
    useMaterial3: false,
    fontFamily: kFontStack,
    scaffoldBackgroundColor: kBg,
    colorScheme: const ColorScheme.light(
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
          titleLarge: const TextStyle(fontSize: 20, fontWeight: FontWeight.w600, letterSpacing: .3, color: kFg),
          titleMedium: const TextStyle(fontSize: 14.5, fontWeight: FontWeight.w600, letterSpacing: .2, color: kFg),
          bodyMedium: const TextStyle(fontSize: 14, color: kFg),
          bodySmall: const TextStyle(fontSize: 11.5, color: kMut),
        ),
    platform: TargetPlatform.android,
  );
}