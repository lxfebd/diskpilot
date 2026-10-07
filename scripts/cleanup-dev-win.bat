@echo off
rem DiskPilot 开发收工清理（Windows）
rem 杀掉 dev 会话的残留进程：diskpilot.exe 主程序、占用 1420 端口的 vite dev server。
rem 只精确匹配 DiskPilot 相关（diskpilot / 1420），不碰其他 node 进程。
setlocal

echo [DiskPilot] 清理开发残留...
taskkill /F /IM diskpilot.exe >nul 2>&1 && echo   - 已终止 diskpilot.exe || echo   - 无 diskpilot.exe 进程

rem 释放 vite dev server 端口（1420）
for /f "tokens=5" %%p in ('netstat -ano ^| findstr ":1420" ^| findstr "LISTENING"') do (
    taskkill /F /PID %%p >nul 2>&1 && echo   - 已终止 1420 端口进程 PID=%%p
)

echo [DiskPilot] 清理完成。检查：tasklist | findstr diskpilot  应无输出。
endlocal
