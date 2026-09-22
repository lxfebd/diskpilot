using System;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using System.Threading;

// Elevate — 提权桥：非管理员时用 ShellExecuteEx(runas) 重跑子命令，
// 管理员子进程把 stdout 重定向到临时文件，父进程轮询读回打印。
//
// 用途：CPU 温度（MSR/IA32_THERM_STATUS）与完整传感器快照需要 Ring0 驱动 +
// 管理员令牌。`fancmd elevate sensors` 在非管理员下自动弹一次 UAC，
// 拿到全量数据后原样回传，调用方无感知。
//
// 设计要点：
//   - 子进程用 `--capture <tmp>` 前缀参数重跑同一个命令，管理员令牌下
//     LHM 自动加载内核驱动（WinRing0/inpout），CPU 温度传感器就有值了。
//   - 已在管理员下直接 CreateProcess（不弹 UAC），输出同样走捕获文件。
//   - 用户取消 UAC → 返回非零退出码 + 中文错误，调用方如实降级。
static class Elevate {
    const uint GENERIC_READ_WRITE = 0xC0000000;
    const uint OPEN_EXISTING = 3;
    const uint FILE_ATTRIBUTE_NORMAL = 0x80;
    const uint SEE_MASK_NOCLOSEPROCESS = 0x00000040;
    const uint SW_SHOWNORMAL = 1;
    const int WAIT_TIMEOUT = 0x00000102;

    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr CreateFileW(string lpFileName, uint dwDesiredAccess, uint dwShareMode,
        IntPtr lpSecurityAttributes, uint dwCreationDisposition, uint dwFlagsAndAttributes, IntPtr hTemplateFile);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool CloseHandle(IntPtr h);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern uint WaitForSingleObject(IntPtr hHandle, uint dwMilliseconds);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool GetExitCodeProcess(IntPtr hProcess, out uint lpExitCode);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    struct SHELLEXECUTEINFOW {
        public uint cbSize;
        public uint fMask;
        public IntPtr hwnd;
        [MarshalAs(UnmanagedType.LPWStr)] public string lpVerb;
        [MarshalAs(UnmanagedType.LPWStr)] public string lpFile;
        [MarshalAs(UnmanagedType.LPWStr)] public string lpParameters;
        [MarshalAs(UnmanagedType.LPWStr)] public string lpDirectory;
        public int nShow;
        public IntPtr hInstApp;
        public IntPtr lpIDList;
        [MarshalAs(UnmanagedType.LPWStr)] public string lpClass;
        public IntPtr hkeyClass;
        public uint dwHotKey;
        public IntPtr hIcon;
        public IntPtr hProcess;
    }
    [DllImport("shell32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool ShellExecuteExW(ref SHELLEXECUTEINFOW pExecInfo);

    public static bool IsAdmin() {
        try {
            using var id = WindowsIdentity.GetCurrent();
            return new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator);
        } catch { return false; }
    }

    static string QuoteArg(string s) {
        if (string.IsNullOrEmpty(s)) return "\"\"";
        if (s.IndexOfAny(new[] { ' ', '"' }) < 0) return s;
        return "\"" + s.Replace("\"", "\\\"") + "\"";
    }

    /// <summary>运行 `elevate <子命令> [参数…]`：非管理员提权重跑，管理员直跑；输出统一捕获回传。</summary>
    public static int Run(string[] cmds) {
        var rest = cmds.Length > 1 ? cmds[1..] : Array.Empty<string>();
        if (rest.Length == 0 || (rest[0] != "sensors" && rest[0] != "list" && rest[0] != "ls"
            && rest[0] != "diag" && rest[0] != "superio" && rest[0] != "write")) {
            Console.Error.WriteLine("用法: fancmd elevate sensors|list|diag|superio|write <args…>");
            return 2;
        }
        var exe = Process.GetCurrentProcess().MainModule?.FileName;
        if (string.IsNullOrEmpty(exe) || !File.Exists(exe)) {
            Console.Error.WriteLine("无法定位 fancmd.exe 自身路径。");
            return 2;
        }
        var tmp = Path.Combine(Path.GetTempPath(), "fancmd_el_" + Guid.NewGuid().ToString("N") + ".txt");
        var childArgs = string.Join(" ", new[] { "--capture", tmp }.Concat(rest).Select(QuoteArg));

        bool ran = false;
        uint exitCode = 1;
        if (IsAdmin()) {
            var psi = new ProcessStartInfo(exe, childArgs) {
                UseShellExecute = false,
                RedirectStandardOutput = false,
                CreateNoWindow = true,
            };
            try {
                var p = Process.Start(psi);
                p?.WaitForExit(120000);
                ran = true;
            } catch (Exception ex) {
                Console.Error.WriteLine("启动子进程失败：" + ex.Message);
                ran = false;
            }
        } else {
            var sei = new SHELLEXECUTEINFOW {
                cbSize = (uint)Marshal.SizeOf<SHELLEXECUTEINFOW>(),
                fMask = SEE_MASK_NOCLOSEPROCESS,
                lpVerb = "runas",
                lpFile = exe,
                lpParameters = childArgs,
                lpDirectory = Path.GetDirectoryName(exe) ?? Environment.SystemDirectory,
                nShow = (int)SW_SHOWNORMAL,
            };
            // ShellExecuteExW(runas) 在 UAC 弹窗等待期间是**同步阻塞**的——
            // 必须放后台线程，主线程只等有限时长。UAC 被确认 → 子进程跑完
            // 写 capture → 主线程读回全量；无人确认 → 快速降级返回（AI 调用不卡死）。
            var worker = new Thread(() => {
                try { ran = ShellExecuteExW(ref sei); }
                catch { ran = false; }
                if (ran && sei.hProcess != IntPtr.Zero) {
                    // 子进程（管理员 fancmd sensors）跑 LHM 枚举 + 写 capture，约需 3-6s
                    WaitForSingleObject(sei.hProcess, 6000);
                    GetExitCodeProcess(sei.hProcess, out var code);
                    exitCode = code;
                    CloseHandle(sei.hProcess);
                }
            }) { IsBackground = true };
            worker.Start();
            if (!worker.Join(9000)) {
                Console.Error.WriteLine("提权等待超时（UAC 未在 9s 内确认）。非管理员下 CPU 温度（MSR）不可读，GPU/内存等传感器仍可读。");
                Console.Error.WriteLine("提示：以管理员身份运行 DiskPilot 后可免 UAC 直达全量传感器。");
                return 1;
            }
            if (!ran) {
                Console.Error.WriteLine("提权失败（用户取消或系统拒绝 UAC）。非管理员下 CPU 温度（MSR）不可读，GPU/内存等传感器仍可读。");
                Console.Error.WriteLine("提示：以管理员身份运行 DiskPilot 后可免 UAC 直达全量传感器。");
                return 1;
            }
        }
        if (!ran) return 1;

        // 读回捕获文件（子进程退出后文件已写完）
        string output = "";
        for (int i = 0; i < 25; i++) {
            try {
                if (File.Exists(tmp)) { output = File.ReadAllText(tmp, Encoding.UTF8); break; }
            } catch { }
            Thread.Sleep(200);
        }
        try { if (File.Exists(tmp)) File.Delete(tmp); } catch { }
        if (!string.IsNullOrEmpty(output)) {
            Console.Out.Write(output);
            Console.Out.Flush();
            return (int)exitCode;
        }
        Console.Error.WriteLine($"子进程退出码 {exitCode}，未产生输出（可能被 UAC 拒绝或无传感器数据）。");
        return (int)exitCode;
    }
}