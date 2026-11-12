; ═══════════════════════════════════════════════════════════════
; QuickCopy 自定义 NSIS 安装脚本
; 替代 Tauri 原生 NSIS 完成页：增加「开机自启」选项页（默认勾选 = 静默模式），
; 勾选写入注册表 Run 键 `"<exe>" --hidden`（开机后台运行仅托盘）。
; 用法（在 installer/ 目录下）：
;   makensis -DVERSION=x.y.z QuickCopy.nsi
; 产物：..\dist-release\QuickCopy_<version>_x64-setup.exe
; ═══════════════════════════════════════════════════════════════

Unicode true
CRCCheck on
SetCompressor /SOLID lzma

!ifndef VERSION
  !define VERSION "0.3.0"
!endif

!define PRODUCT_NAME "快捷粘贴 QuickCopy"
!define APP_NAME "QuickCopy"
!define EXE_NAME "QuickCopy.exe"
!define RUN_KEY "Software\Microsoft\Windows\CurrentVersion\Run"
!define UNINST_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\QuickCopy"
!define SRC_EXE "..\src-tauri\target\release\QuickCopy.exe"
!define SRC_DLL "..\src-tauri\WebView2Loader.dll"
!define SRC_ICON "..\src-tauri\icons\icon.ico"

Name "${PRODUCT_NAME}"
OutFile "..\dist-release\QuickCopy_${VERSION}_x64-setup.exe"
InstallDir "$LOCALAPPDATA\QuickCopy"
InstallDirRegKey HKCU "${UNINST_KEY}" "InstallLocation"
RequestExecutionLevel user
InstallButtonText "安装(&I)"

!include "MUI2.nsh"
!include "nsDialogs.nsh"
!include "LogicLib.nsh"

!define MUI_ICON "${SRC_ICON}"
!define MUI_UNICON "${SRC_ICON}"
!define MUI_ABORTWARNING
!define MUI_FINISHPAGE_RUN "$INSTDIR\QuickCopy.exe"
!define MUI_FINISHPAGE_RUN_TEXT "立即运行 ${PRODUCT_NAME}"
!define MUI_FINISHPAGE_RUN_PARAMETERS ""

!insertmacro MUI_PAGE_WELCOME
!insertmacro MUI_PAGE_DIRECTORY
Page custom BootPageCreate BootPageLeave "开机自启"
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "SimpChinese"

; ═══════════ 开机自启选项页（默认勾选 = 静默模式） ═══════════
Var BootAutoCheck
Var DeskShortcutCheck

Function BootPageCreate
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}
  ${NSD_CreateLabel} 0 0 100% 30u "是否让 ${PRODUCT_NAME} 随 Windows 登录自动启动？"
  Pop $0
  ${NSD_CreateLabel} 0 30u 100% 20u "默认勾选为静默模式：开机后后台运行，仅显示托盘图标。"
  Pop $0
  ${NSD_CreateCheckBox} 0 62u 100% 24u "开机自启（静默模式）"
  Pop $BootAutoCheck
  ${NSD_SetState} $BootAutoCheck ${BST_CHECKED}
  ${NSD_CreateCheckBox} 0 80u 100% 20u "创建桌面快捷方式（同时创建开始菜单快捷方式）"
  Pop $DeskShortcutCheck
  ${NSD_SetState} $DeskShortcutCheck ${BST_CHECKED}
  nsDialogs::Show
FunctionEnd

Function BootPageLeave
  ; 勾选状态在 Section 中读取（Leave 里不做事，静默安装时静默默认勾选）
FunctionEnd

; ═══════════ 安装 ═══════════
Section "主程序" SecMain
  SectionIn RO
  ; 覆盖安装前先结束运行中的旧实例（文件被占用会失败）
  nsExec::ExecToLog 'taskkill /F /IM QuickCopy.exe'
  SetOutPath "$INSTDIR"
  File "${SRC_EXE}"
  File "${SRC_DLL}"

  ; 开机自启：静默安装（/S，页面未显示）也默认勾选
  ${If} $BootAutoCheck != ""
    ${NSD_GetState} $BootAutoCheck $0
  ${Else}
    StrCpy $0 ${BST_CHECKED}
  ${EndIf}
  ${If} $0 == ${BST_CHECKED}
    WriteRegStr HKCU "${RUN_KEY}" "${APP_NAME}" '"$INSTDIR\${EXE_NAME}" --hidden'
  ${Else}
    DeleteRegValue HKCU "${RUN_KEY}" "${APP_NAME}"
  ${EndIf}

  ; 卸载程序与卸载信息
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayName" "${PRODUCT_NAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "${UNINST_KEY}" "DisplayIcon" "$INSTDIR\${EXE_NAME}"
  WriteRegStr HKCU "${UNINST_KEY}" "UninstallString" '"$INSTDIR\Uninstall.exe"'
  WriteRegStr HKCU "${UNINST_KEY}" "InstallLocation" "$INSTDIR"
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINST_KEY}" "NoRepair" 1

  ; 桌面快捷方式：安装选项页勾选（静默安装默认创建）
  ${If} $DeskShortcutCheck != ""
    ${NSD_GetState} $DeskShortcutCheck $0
  ${Else}
    StrCpy $0 ${BST_CHECKED}
  ${EndIf}
  ${If} $0 == ${BST_CHECKED}
    CreateShortCut "$DESKTOP\${PRODUCT_NAME}.lnk" "$INSTDIR\${EXE_NAME}" "" "$INSTDIR\${EXE_NAME}" 0
  ${EndIf}

  ; 开始菜单快捷方式
  CreateDirectory "$SMPROGRAMS\${PRODUCT_NAME}"
  CreateShortCut "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk" "$INSTDIR\${EXE_NAME}" "" "$INSTDIR\${EXE_NAME}" 0
SectionEnd

; ═══════════ 卸载 ═══════════
; 卸载段必须写 "Section Uninstall"（安装阶段不会执行）
Section Uninstall
  nsExec::ExecToLog 'taskkill /F /IM QuickCopy.exe'
  DeleteRegValue HKCU "${RUN_KEY}" "${APP_NAME}"
  DeleteRegKey HKCU "${UNINST_KEY}"
  Delete "$DESKTOP\${PRODUCT_NAME}.lnk"
  Delete "$SMPROGRAMS\${PRODUCT_NAME}\${PRODUCT_NAME}.lnk"
  RMDir "$SMPROGRAMS\${PRODUCT_NAME}"
  RMDir /r "$INSTDIR"
SectionEnd