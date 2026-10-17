@echo off
chcp 65001 >nul
cd /d "%~dp0"
echo ============================================
echo   Token 用量实时仪表盘
echo   （每次刷新页面自动重新采集最新数据）
echo ============================================
python serve.py
if errorlevel 1 (
    echo.
    echo  启动失败。请确认已安装 Python 3，或在新开的
    echo  终端中运行：py -3 serve.py
    pause
)
