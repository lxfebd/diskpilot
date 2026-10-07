@echo off
rem DiskPilot 一键开发启动（Windows）
rem 自动处理：cargo 不在 PATH（tauri 需要它）+ UAC 提权清单（DISKPILOT_NO_ADMIN=1）。
rem 用法：双击运行，或命令行执行 scripts\dev-win.bat
setlocal
cd /d "%~dp0..\apps\desktop"

rem 把用户 cargo 目录加进 PATH（tauri dev 要调 cargo metadata）
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

rem 跳过 UAC 提权清单（os error 740 的根因；工具墙部分写工具会降级提示）
set "DISKPILOT_NO_ADMIN=1"

echo [DiskPilot] 启动中... 停止请直接关窗口或 Ctrl+C
call pnpm tauri dev
endlocal
