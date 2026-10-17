@echo off
chcp 65001 >nul
echo ============================================
echo   正在停止 Token 用量仪表盘服务 (端口 8756)
echo ============================================
set FOUND=0
for /f "tokens=5" %%a in ('netstat -ano ^| findstr ":8756" ^| findstr "LISTENING"') do (
    taskkill /pid %%a /f >nul 2>&1
    set FOUND=1
    echo   已结束进程 PID %%a
)
if "%FOUND%"=="0" (
    echo   未发现正在运行的服务。
)
echo.
echo  完成。如需再次启动，请运行 启动仪表盘.bat
pause