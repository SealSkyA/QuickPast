# -*- coding: utf-8 -*-
"""
QuickCopy 版本资源注入脚本（构建后后处理，Windows 专用）

作用：
    tauri-build 1.x（tauri-winres 0.1）生成的版本资源缺少 VarFileInfo/Translation，
    Windows 资源管理器读不到 FileVersion/ProductName（Get-Item .VersionInfo 全空）。
    本脚本用 kernel32 的资源 API（BeginUpdateResourceW / UpdateResourceW /
    EndUpdateResourceW，Python ctypes 直调，无需任何资源编译器/工具链），
    在编译好的 exe 上叠加写入一份带 0x0409 语言的完整 RT_VERSION 资源。

用法：
    python scripts/qp_fix_version.py <exe路径> [版本号]
    例：python scripts/qp_fix_version.py "src-tauri\\target\\release\\QuickCopy.exe" 0.2.18
"""
import ctypes
import struct
import sys


def wstr(s):
    """Unicode 字符串 + 结尾 null（UTF-16LE）"""
    return s.encode("utf-16-le") + b"\x00\x00"


def pad4(buf):
    while len(buf) % 4:
        buf.append(0)


def blk_start(buf):
    pad4(buf)
    return len(buf)


def blk_hdr(buf, value_len, wtype, key):
    # 块头：wLength 占位 + wValueLength + wType + 键名（null 结尾），后补 4 对齐
    buf += struct.pack("<HHH", 0, value_len, wtype)
    buf += wstr(key)
    pad4(buf)


def blk_end(buf, start):
    struct.pack_into("<H", buf, start, len(buf) - start)


def build_version(v1, v2, v3, v4, props, lang=0x0409, charset=0x04B0):
    """构造标准 VS_VERSIONINFO 二进制块（Win32 版本资源格式）"""
    buf = bytearray()
    root = blk_start(buf)
    # ---- VS_VERSIONINFO 根块 ----
    buf += struct.pack("<HHH", 0, 52, 0)  # wLength 占位 / wValueLength=52 / wType=0
    buf += wstr("VS_VERSION_INFO")
    pad4(buf)
    # ---- VS_FIXEDFILEINFO（52 字节）----
    fms = (v1 << 16) | v2  # fileVersionMS / productVersionMS
    fls = (v3 << 16) | v4  # fileVersionLS / productVersionLS
    fixed = bytearray()
    fixed += struct.pack("<I", 0xFEEF04BD)  # dwSignature
    fixed += struct.pack("<I", 0x00010000)  # dwStrucVersion
    fixed += struct.pack("<I", fms)  # dwFileVersionMS
    fixed += struct.pack("<I", fls)  # dwFileVersionLS
    fixed += struct.pack("<I", fms)  # dwProductVersionMS
    fixed += struct.pack("<I", fls)  # dwProductVersionLS
    fixed += struct.pack("<I", 0x3F)  # dwFileFlagsMask
    fixed += struct.pack("<I", 0)  # dwFileFlags
    fixed += struct.pack("<I", 0x00040004)  # dwFileOS = VOS_NT_WINDOWS32
    fixed += struct.pack("<I", 1)  # dwFileType = VFT_APP
    fixed += struct.pack("<I", 0)  # dwFileSubtype
    fixed += struct.pack("<I", 0)  # dwFileDateMS
    fixed += struct.pack("<I", 0)  # dwFileDateLS
    assert len(fixed) == 52, "VS_FIXEDFILEINFO 必须 52 字节"
    buf += fixed
    pad4(buf)
    # ---- StringFileInfo ----
    sfi = blk_start(buf)
    blk_hdr(buf, 0, 1, "StringFileInfo")
    st = blk_start(buf)
    blk_hdr(buf, 0, 1, "%04x%04x" % (lang, charset))  # e.g. 040904B0
    for k, v in props.items():
        s = blk_start(buf)
        blk_hdr(buf, (len(v) + 1) * 2, 1, k)
        buf += wstr(v)
        pad4(buf)
        blk_end(buf, s)
    blk_end(buf, st)
    blk_end(buf, sfi)
    # ---- VarFileInfo（缺这块正是 Windows 读不到版本的根因）----
    vfi = blk_start(buf)
    blk_hdr(buf, 0, 1, "VarFileInfo")
    var = blk_start(buf)
    blk_hdr(buf, 4, 0, "Translation")
    buf += struct.pack("<I", lang)
    buf += struct.pack("<I", charset)
    blk_end(buf, var)
    blk_end(buf, vfi)
    blk_end(buf, root)
    return bytes(buf)


def inject(exe, data, lang=0x0409):
    k32 = ctypes.windll.kernel32
    k32.BeginUpdateResourceW.restype = ctypes.c_void_p
    k32.BeginUpdateResourceW.argtypes = [ctypes.c_wchar_p, ctypes.c_bool]
    k32.UpdateResourceW.restype = ctypes.c_bool
    k32.UpdateResourceW.argtypes = [
        ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
        ctypes.c_ushort, ctypes.c_void_p, ctypes.c_ulong,
    ]
    k32.EndUpdateResourceW.restype = ctypes.c_bool
    k32.EndUpdateResourceW.argtypes = [ctypes.c_void_p, ctypes.c_bool]

    h = k32.BeginUpdateResourceW(exe, False)
    if not h:
        raise ctypes.WinError()
    try:
        buf = ctypes.create_string_buffer(data, len(data))
        ok = k32.UpdateResourceW(  # RT_VERSION=16, 名称=1
            h, ctypes.c_void_p(16), ctypes.c_void_p(1), lang, buf, len(data))
        if not ok:
            raise ctypes.WinError()
        if not k32.EndUpdateResourceW(h, False):
            raise ctypes.WinError()
    except Exception:
        k32.EndUpdateResourceW(h, True)
        raise


def default_props(version):
    return {
        "CompanyName": "QuickCopy",
        "FileDescription": "QuickCopy —— 局域网剪贴板历史与文件互传",
        "FileVersion": version,
        "InternalName": "QuickCopy.exe",
        "OriginalFilename": "QuickCopy.exe",
        "ProductName": "QuickCopy",
        "ProductVersion": version,
    }


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("用法: python qp_fix_version.py <exe路径> [版本号]", file=sys.stderr)
        sys.exit(2)
    exe = sys.argv[1]
    version = sys.argv[2] if len(sys.argv) > 2 else "0.2.18"
    parts = [int(x) for x in version.split(".")]
    while len(parts) < 4:
        parts.append(0)
    data = build_version(*parts[:4], props=default_props(version))
    inject(exe, data)
    print("RT_VERSION(%s) 已注入 %s" % (version, exe))