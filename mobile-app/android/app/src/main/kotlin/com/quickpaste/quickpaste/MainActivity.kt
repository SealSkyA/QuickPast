package com.quickpaste.quickpaste

import android.content.Context
import android.content.Intent
import android.net.wifi.WifiManager
import androidx.core.content.FileProvider
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel
import java.io.File

class MainActivity : FlutterActivity() {
    private var multicastLock: WifiManager.MulticastLock? = null
    private var highPerfLock: WifiManager.WifiLock? = null

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        // 组播锁 + 高性能锁：Android 默认丢弃组播报文，不获取锁则收不到局域网设备广播
        // （表现为「首次能发现、后台回来搜不到」）。后台可能被系统释放，恢复前台时重取。
        // WIFI_MODE_FULL_HIGH_PERF：传输大文件期间强制 WiFi 满功率，关闭 PSM 省电。
        // 不设会表现为局域网 RTT 100ms+、大流量 TCP 吞吐崩溃（小米/Redmi 等 OEM 尤甚）。
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "qp_wifi").setMethodCallHandler { call, result ->
            when (call.method) {
                "acquireMulticast" -> {
                    try {
                        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                        if (multicastLock == null) {
                            multicastLock = wm.createMulticastLock("QuickCopy").apply { setReferenceCounted(false) }
                        }
                        multicastLock?.acquire()
                        result.success(true)
                    } catch (e: Exception) {
                        result.success(false) // 无 Wi-Fi/权限异常时静默降级（单播回执链路仍可用）
                    }
                }
                "acquireHighPerf" -> {
                    try {
                        val wm = applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
                        if (highPerfLock == null) {
                            highPerfLock = wm.createWifiLock(
                                WifiManager.WIFI_MODE_FULL_HIGH_PERF, "QuickCopy")
                                .apply { setReferenceCounted(false) }
                        }
                        highPerfLock?.acquire()
                        result.success(true)
                    } catch (e: Exception) {
                        result.success(false)
                    }
                }
                "releaseHighPerf" -> {
                    try {
                        highPerfLock?.let { if (it.isHeld) it.release() }
                        result.success(true)
                    } catch (e: Exception) {
                        result.success(false)
                    }
                }
                else -> result.notImplemented()
            }
        }

        // 打开目录：FileProvider + ACTION_VIEW 直达系统文件管理器
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "qp_open").setMethodCallHandler { call, result ->
            when (call.method) {
                "openDir" -> openDir((call.arguments as? String) ?: "", result)
                else -> result.notImplemented()
            }
        }
    }

    private fun openDir(path: String, result: MethodChannel.Result) {
        try {
            val file = File(path)
            val uri = FileProvider.getUriForFile(this, "$packageName.fileprovider", file)
            val intent = Intent(Intent.ACTION_VIEW)
            intent.setDataAndType(uri, "resource/folder")
            intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            startActivity(intent)
            result.success(true)
        } catch (e: Exception) {
            // 无文件管理器/路径无效 → SAF 文档树选择器兜底（用户可自行导航到保存目录）
            try {
                val intent = Intent(Intent.ACTION_OPEN_DOCUMENT_TREE)
                intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                startActivity(intent)
                result.success(true)
            } catch (e2: Exception) {
                result.error("open_dir", e2.message, null)
            }
        }
    }
}
