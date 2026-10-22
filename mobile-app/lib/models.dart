// 数据模型：设备 / 收发会话 / 历史条目 / 设置
import 'dart:convert';

/// 局域网设备（对齐 PC 端 Device 结构）
class Device {
  final String fingerprint;
  final String alias;
  final String deviceType;
  final String version;
  final String ip;
  final int port;
  final String protocol;
  final bool download;
  int lastSeen; // epoch ms

  Device({
    required this.fingerprint,
    required this.alias,
    required this.deviceType,
    required this.version,
    required this.ip,
    required this.port,
    required this.protocol,
    required this.download,
    required this.lastSeen,
  });

  factory Device.fromAnnounce(Map<String, dynamic> v, String fromIp) {
    return Device(
      fingerprint: (v['fingerprint'] as String?) ?? '',
      alias: (v['alias'] as String?) ?? '?',
      deviceType: (v['deviceType'] as String?) ?? 'mobile',
      version: (v['version'] as String?) ?? '?',
      ip: fromIp,
      port: ((v['port'] as num?) ?? 53318).toInt(),
      protocol: (v['protocol'] as String?) ?? 'http',
      download: (v['download'] as bool?) ?? false,
      lastSeen: DateTime.now().millisecondsSinceEpoch,
    );
  }

  factory Device.fromJson(Map<String, dynamic> v) {
    return Device(
      fingerprint: (v['fingerprint'] as String?) ?? '',
      alias: (v['alias'] as String?) ?? '?',
      deviceType: (v['deviceType'] as String?) ?? 'mobile',
      version: (v['version'] as String?) ?? '?',
      ip: (v['ip'] as String?) ?? '',
      port: ((v['port'] as num?) ?? 53318).toInt(),
      protocol: (v['protocol'] as String?) ?? 'http',
      download: (v['download'] as bool?) ?? false,
      lastSeen: ((v['lastSeen'] as num?) ?? 0).toInt(),
    );
  }

  Map<String, dynamic> toJson() => {
        'fingerprint': fingerprint,
        'alias': alias,
        'deviceType': deviceType,
        'version': version,
        'ip': ip,
        'port': port,
        'protocol': protocol,
        'download': download,
        'lastSeen': lastSeen,
      };

  bool get isSelf => fingerprint.isEmpty;
}

/// prepare-upload 中声明的一个文件
class IncomingFile {
  final String fileId;
  final String fileName;
  final int size;
  final String fileType;
  final String? sha256;
  String? token;
  String state; // Pending | InProgress | Finished | Failed
  String? path;
  int written = 0;

  IncomingFile({
    required this.fileId,
    required this.fileName,
    required this.size,
    required this.fileType,
    this.sha256,
    this.token,
    this.state = 'Pending',
    this.path,
  });

  factory IncomingFile.fromJson(String fileId, Map<String, dynamic> v) {
    return IncomingFile(
      fileId: fileId,
      fileName: (v['fileName'] as String?) ?? 'unnamed',
      size: ((v['size'] as num?) ?? 0).toInt(),
      fileType: (v['fileType'] as String?) ?? 'application/octet-stream',
      sha256: (v['sha256'] as String?)?.isNotEmpty == true ? v['sha256'] as String : null,
    );
  }

  bool get isText => fileType == 'text';
}

/// 接收会话（单发件人一批文件）
class IncomingSession {
  final String sessionId;
  final String senderIp;
  final String senderAlias;
  final List<IncomingFile> files;
  String state; // Pending | Accepted | Rejected | Finished | Canceled
  final DateTime createdAt;

  IncomingSession({
    required this.sessionId,
    required this.senderIp,
    required this.senderAlias,
    required this.files,
    required this.state,
    required this.createdAt,
  });

  int get totalSize => files.fold(0, (a, f) => a + f.size);
  bool get allDone => files.every((f) => f.state == 'Finished' || f.state == 'Failed');
}

/// 发送条目（选择器/文本生成的待发送对象）
class SendItem {
  final String name;
  final int size;
  final String fileType; // file 使用 mime 或 application/octet-stream；文本为 text
  final String? localPath; // 文本为 null
  final String? text;

  SendItem({required this.name, required this.size, required this.fileType, this.localPath, this.text});

  /// 文本条目（body 直传，不落盘）
  bool get isText => text != null;

  /// 文件条目（流式上传）
  bool get isFileLike => localPath != null;

  Map<String, dynamic> toPrepareJson(String id) => {
        'id': id,
        'fileName': name,
        'size': size,
        'fileType': fileType,
      };
}

/// 历史记录条目（内存态；显示用）
class HistoryEntry {
  final bool incoming;
  final String peerAlias;
  final String summary;
  final String detail; // 字节数/文本预览
  final DateTime time;
  final bool ok;

  HistoryEntry({
    required this.incoming,
    required this.peerAlias,
    required this.summary,
    required this.detail,
    required this.time,
    required this.ok,
  });
}

/// 格式化字节
String fmtBytes(int n) {
  if (n < 1024) return '$n B';
  if (n < 1024 * 1024) return '${(n / 1024).toStringAsFixed(1)} KB';
  if (n < 1024 * 1024 * 1024) return '${(n / 1024 / 1024).toStringAsFixed(1)} MB';
  return '${(n / 1024 / 1024 / 1024).toStringAsFixed(2)} GB';
}

String jsonPretty(Object v) => const JsonEncoder.withIndent(' ').convert(v);