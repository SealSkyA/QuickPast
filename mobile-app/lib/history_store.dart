// 内存历史记录（收发记录；应用会话内保留，使用 ChangeNotifier 通知界面刷新）
import 'package:flutter/foundation.dart';

import 'models.dart';

class HistoryStore extends ChangeNotifier {
  HistoryStore._();
  static final HistoryStore instance = HistoryStore._();

  final List<HistoryEntry> _entries = [];

  List<HistoryEntry> get entries => List.unmodifiable(_entries);

  void add(HistoryEntry e) {
    _entries.insert(0, e);
    if (_entries.length > 200) _entries.removeLast();
    notifyListeners();
  }

  void clear() {
    if (_entries.isEmpty) return;
    _entries.clear();
    notifyListeners();
  }
}