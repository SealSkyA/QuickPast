@echo off
rem QuickPaste dev prerequisite: install VS Build Tools (VC toolchain + Win11 SDK)
rem Usage: double-click this file -> click Yes on UAC -> wait for the black window to close
cd /d "%~dp0"
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install-vs-buildtools.ps1"