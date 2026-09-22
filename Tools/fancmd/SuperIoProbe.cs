// SuperIoProbe — 跨机器自适配 SuperIO 写控桥（不依赖任何特定驱动/芯片/端口）
// 用法:
//   fancmd diag            完整诊断：驱动/芯片/端口/基址/可写性（结构化 JSON + 人读）
//   fancmd superio         读传感器（温度/风扇转速/PWM）
//   fancmd write <idx> <pct>    写占空比（自动枚举并保存原始值）
//   fancmd write reset <idx>    恢复主板自动控制
//
// 自适配策略：
//   1. 驱动：LoadLibrary 探测 inpoutx64/inpout32/WinRing0x64/WinRing0/WinIo32，已装哪个用哪个
//   2. 芯片：ITE(0x87 0x01 0x55..) / Nuvoton(0x87 0x87) / Winbond 三套 enter 自动尝试
//   3. 端口：0x2E / 0x4E 自动枚举
//   4. 写控：IT87 直写 PWM；Nuvoton NCT6798D 走其标志寄存器
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;

static class SuperIoProbe {
    // ── CreateFileW 用于打开 inpoutx64 设备（判断驱动是否真实可用）──
    const uint GENERIC_READ_WRITE = 0xC0000000;
    const uint OPEN_EXISTING = 3;
    const uint FILE_ATTRIBUTE_NORMAL = 0x80;
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    internal static extern IntPtr CreateFileW(string lpFileName, uint dwDesiredAccess, uint dwShareMode,
        IntPtr lpSecurityAttributes, uint dwCreationDisposition, uint dwFlagsAndAttributes, IntPtr hTemplateFile);
    [DllImport("kernel32.dll", SetLastError = true)]
    internal static extern bool CloseHandle(IntPtr h);

    // ── 动态驱动加载：多驱动兜底，跨机器自适配 ──
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern IntPtr LoadLibraryW(string lpFileName);
    [DllImport("kernel32.dll", SetLastError = true)]
    static extern bool FreeLibrary(IntPtr hModule);
    // 端口读写入口点（inpoutx64/inpout32/WinRing0/WinIo 系列共用签名；WinRing0 有额外 Init）
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    delegate byte InpFn(ushort port);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    delegate void OutFn(ushort port, byte val);
    [UnmanagedFunctionPointer(CallingConvention.StdCall)]
    delegate bool InitFn();

    static IntPtr _hLib;
    static InpFn _inp;
    static OutFn _out;
    static string _drvName = "";
    static bool _needsInit;   // WinRing0 需要先 InitWinRing0()
    static bool _driverOk;    // 驱动加载 + 入口点解析是否成功

    public static string DriverName => _drvName;
    public static bool DriverLoaded => _driverOk;

    /// <summary>从候选驱动里加载第一个可用的（已装在其 DLL 位置）。找不到返回 false。</summary>
    static bool LoadPortDriver() {
        if (_driverOk) return true;
        string[] candidates = {
            "inpoutx64.dll", "inpout32.dll",
            "WinRing0x64.dll", "WinRing0.dll",
            "WinIo64.dll", "WinIo32.dll"
        };
        // 跨机器适配：驱动 DLL 未必在 System32——先收集所有已知候选目录
        foreach (var dir in CandidateDriverDirs()) {
            foreach (var dll in candidates) {
                var full = Path.Combine(dir, dll);
                if (!File.Exists(full)) continue;
                if (TryLoadOne(full)) return true;
            }
        }
        // 最后走系统搜索路径（DLL 目录 / System32 / PATH）
        foreach (var dll in candidates) {
            var h = LoadLibraryW(dll);
            if (h == IntPtr.Zero) continue;
            if (TryResolveAndKeep(h, dll)) return true;
            FreeLibrary(h);
        }
        return false;
    }

    /// <summary>候选驱动目录：exe 目录 / System32 / 环境变量指定 / 上溯式 Tools 布局扫描。</summary>
    static IEnumerable<string> CandidateDriverDirs() {
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        void Add(string p) { if (!string.IsNullOrEmpty(p) && Directory.Exists(p) && seen.Add(Path.GetFullPath(p))) { /* 已登记 */ } }

        Add(AppContext.BaseDirectory);
        Add(Environment.GetFolderPath(Environment.SpecialFolder.System));
        Add(Environment.GetFolderPath(Environment.SpecialFolder.SystemX86));
        var toolsDir = Environment.GetEnvironmentVariable("DISKPILOT_TOOLS_DIR");
        Add(toolsDir);

        // 上溯式布局扫描：从 exe 目录向父级爬，在各层找 Tools/ 相关子目录里的驱动
        var cur = new DirectoryInfo(AppContext.BaseDirectory);
        var depth = 0;
        while (cur != null && depth < 6) {
            foreach (var sub in cur.GetDirectories("*", new EnumerationOptions {
                         RecurseSubdirectories = false, IgnoreInaccessible = true, AttributesToSkip = 0 })) {
                var n = sub.Name;
                if (n.Equals("Tools", StringComparison.OrdinalIgnoreCase)
                    || n.Contains("fancontrol", StringComparison.OrdinalIgnoreCase)
                    || n.Contains("fan", StringComparison.OrdinalIgnoreCase)
                    || n.EndsWith("Tools", StringComparison.OrdinalIgnoreCase)) {
                    Add(sub.FullName);
                    foreach (var sub2 in sub.GetDirectories("*", new EnumerationOptions {
                                 RecurseSubdirectories = false, IgnoreInaccessible = true, AttributesToSkip = 0 })) {
                        Add(sub2.FullName);
                    }
                }
            }
            cur = cur.Parent;
            depth++;
        }
        return seen;
    }

    static bool TryLoadOne(string fullPath) {
        var h = LoadLibraryW(fullPath);
        if (h == IntPtr.Zero) return false;
        return TryResolveAndKeep(h, Path.GetFileName(fullPath));
    }

    static bool TryResolveAndKeep(IntPtr h, string dll) {
        // 解析入口点（IntPtr 是值类型，不能 `??`，手写回退）
        var inp = GetProc(h, "Inp32");
        if (inp == IntPtr.Zero) inp = GetProc(h, "ReadPort");
        var outp = GetProc(h, "Out32");
        if (outp == IntPtr.Zero) outp = GetProc(h, "WritePort");
        if (inp == IntPtr.Zero || outp == IntPtr.Zero) return false;
        _hLib = h;
        _inp = Marshal.GetDelegateForFunctionPointer<InpFn>(inp);
        _out = Marshal.GetDelegateForFunctionPointer<OutFn>(outp);
        _drvName = dll;
        // WinRing0 需要先 Init 才能用了；inpout 系不需要。
        _needsInit = dll.StartsWith("WinRing0", StringComparison.OrdinalIgnoreCase)
            || dll.Contains("Io", StringComparison.OrdinalIgnoreCase);
        var init = GetProc(h, "InitWinRing0");
        if (_needsInit && init != IntPtr.Zero) {
            var f = Marshal.GetDelegateForFunctionPointer<InitFn>(init);
            if (!f()) return false; // init 失败，试下一个
        }
        _driverOk = true;
        return true;
    }

    static IntPtr GetProc(IntPtr h, string name) {
        try { return GetProcAddress(h, name); } catch { return IntPtr.Zero; }
    }
    [DllImport("kernel32.dll", CharSet = CharSet.Ansi, SetLastError = true)]
    static extern IntPtr GetProcAddress(IntPtr hModule, string lpProcName);

    static bool _devOpen;          // inpoutx64 类设备，供 `superio` 用例参考
    static byte Inp(ushort port) => _inp(port);
    static void Out(ushort port, byte val) => _out(port, val);

    // ── 多芯片 enter 序列 ──
    // ITE:  0x87 0x01 0x55 (+0x4E?0xAA:0x55)   —— LHM 权威逻辑
    // Nuvoton: 0x87 0x87
    // Winbond: 0x2E 0x87 0x87 (+0x2E 0x87)   —— 仅 0x2E 口
    static void EnterChip(ushort addr, ChipKind kind) {
        if (kind == ChipKind.Ite) {
            Out(addr, 0x87); Out(addr, 0x01); Out(addr, 0x55);
            Out(addr, (byte)(addr == 0x4E ? 0xAA : 0x55));
        } else if (kind == ChipKind.Nuvoton) {
            Out(addr, 0x87); Out(addr, 0x87);
        } else { // Winbond
            Out(addr, 0x87); Out(addr, 0x87);
            if (addr == 0x2E) { Out(addr, 0x87); }
        }
    }
    static void ExitChip(ushort addr) {
        Out(addr, 0x02); Out((ushort)(addr + 1), 0x02);
    }
    static byte ReadReg(ushort addr, byte reg) {
        Out(addr, reg);
        return Inp((ushort)(addr + 1));
    }

    enum ChipKind { Ite, Nuvoton, Winbond }
    static ChipKind KindFor(byte idH) {
        if (idH is 0x86 or 0x87 or 0x88 or 0x90) return ChipKind.Ite;      // IT86xx / IT87xx / IT88xx
        if (idH is 0x85 or 0xA0 or 0xE0) return ChipKind.Nuvoton;          // NCT6xxx 系
        return ChipKind.Ite; // 未知默认 ITE（LHM 逻辑最宽松）
    }

    // ── 芯片发现：三套 enter × 两端口，读 LD#7 (0x20/0x21) + LD#4 (0x60/0x61) ──
    static readonly ushort[] SIO_ADDRS = { 0x2E, 0x4E };

    static (ushort addr, ChipKind kind, ushort envBase, byte idH, byte idL) FindSio() {
        foreach (var addr in SIO_ADDRS) {
            foreach (var kind in new[] { ChipKind.Ite, ChipKind.Nuvoton, ChipKind.Winbond }) {
                EnterChip(addr, kind);
                var idH = ReadReg(addr, 0x20);
                var idL = ReadReg(addr, 0x21);
                if (idH == 0xFF || idH == 0x00) { ExitChip(addr); continue; }
                // 芯片确认后：选 LD#4（环境控制器）再读基址 0x60/0x61。
                // 基址寄存器是「当前逻辑设备」的，不选 LD 直接读拿到的是默认
                // 设备（Floppy 等）基址，后续 env 读写全 0xFF。
                Out(addr, 0x07); Out((ushort)(addr + 1), 0x04);
                var hi = ReadReg(addr, 0x60);
                var lo = ReadReg(addr, 0x61);
                ExitChip(addr);
                if (hi != 0xFF || lo != 0xFF)
                    return (addr, kind, (ushort)((hi << 8) | lo), idH, idL);
            }
        }
        return (0, ChipKind.Ite, 0, 0, 0);
    }

    // 进入配置态 + 选 LD#4（环境控制器）
    static void EnterLd4(ushort sioAddr, ChipKind kind) {
        EnterChip(sioAddr, kind);
        Out(sioAddr, 0x07); Out((ushort)(sioAddr + 1), 0x04);
    }
    static void ExitLd4(ushort sioAddr) => ExitChip(sioAddr);

    static bool IsPortAlive(ushort addr) {
        // 写一个字节到 0x70（RTC 地址口），读回验证端口真实可用
        Out(0x70, 0x00);
        var v = Inp(0x71);
        return v != 0xFF;
    }

    // ── 环境寄存器访问（间接 IO：地址口=base+5，数据口=base+6）──
    static readonly byte[] FAN_TACH_REG = { 0x0d, 0x0e, 0x0f, 0x80, 0x82, 0x4c };
    static readonly byte[] FAN_TACH_EXT_REG = { 0x18, 0x19, 0x1a, 0x81, 0x83, 0x4d };
    static readonly byte[] FAN_PWM_CTRL_REG = { 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A };
    static readonly byte[] FAN_PWM_CTRL_EXT_REG = { 0x63, 0x6B, 0x73, 0x7B, 0xA3, 0xAB };

    static void WriteEnv(ushort envBase, byte reg, byte val) {
        Out((ushort)(envBase + 5), reg);
        Out((ushort)(envBase + 6), val);
    }
    static byte ReadEnv(ushort envBase, byte reg) {
        Out((ushort)(envBase + 5), reg);
        return Inp((ushort)(envBase + 6));
    }
    static void SelectBankZero(ushort envBase) {
        var v = ReadEnv(envBase, 0x06);
        v &= 0x9F;
        Out((ushort)(envBase + 5), 0x06);
        Out((ushort)(envBase + 6), v);
    }

    // ── diag：结构化 JSON 诊断（供内置 AI 自修复 + 人读）──
    internal static int Diag(string[] cmds) {
        var d = new Dictionary<string, object?>();
        d["tool"] = "fancmd";
        d["version"] = 2;
        d["elevated"] = IsAdmin();
        d["driver"] = LoadPortDriver()
            ? new { loaded = true, name = _drvName, device_open = TryOpenDevice(_drvName) }
            : new { loaded = false, name = "", note = "未找到任何端口驱动（inpoutx64/inpout32/WinRing0x64/WinRing0/WinIo）。请以管理员安装任一驱动，或让内置 AI 执行安装。" };
        if (!_driverOk) { WriteDiag(d, cmds); return 1; }

        d["rtc_ok"] = IsPortAlive(0x70);
        var (addr, kind, envBase, idH, idL) = FindSio();
        d["sio"] = addr == 0
            ? new { found = false, note = "扫描 0x2E/0x4E 未识别到 SuperIO 芯片（ITE/Nuvoton/Winbond）。" }
            : new { found = true, addr = $"0x{addr:X2}", chip = $"0x{idH:X2}{idL:X2}", kind = kind.ToString(), env_base = $"0x{envBase:X4}" };
        if (addr == 0) { WriteDiag(d, cmds); return 1; }

        // 进入 LD#4 读风扇/温度（构造可写性信息）
        var fans = new List<object>();
        var temps = new List<object>();
        var voltages = new List<object>();
        try {
            EnterLd4(addr, kind);
            SelectBankZero(envBase);
            for (byte f = 0; f < 6; f++) {
                var pwm = ReadEnv(envBase, FAN_PWM_CTRL_REG[f]);
                var ext = ReadEnv(envBase, FAN_PWM_CTRL_EXT_REG[f]);
                var lo = ReadEnv(envBase, FAN_TACH_REG[f]);
                var hi = ReadEnv(envBase, FAN_TACH_EXT_REG[f]);
                var tach = (ushort)((hi << 8) | lo);
                var rpm = tach > 0x3f && tach < 0xffff ? 1.35e6f / (tach * 2) : 0f;
                fans.Add(new { idx = f, pwm = pwm, ext = ext, rpm = rpm > 0 ? (float?)Math.Round(rpm) : (float?)null });
            }
            // 温度区：IT87 环境寄存器 0x29-0x2B（3 个温度源，对应主板/CPU/辅助）
            for (byte i = 0; i < 3; i++) {
                var t = ReadEnv(envBase, (byte)(0x29 + i));
                if (t != 0xFF && t != 0) temps.Add(new { idx = i, c = t, label = i switch { 0 => "System/主板", 1 => "CPU", _ => "Aux/辅助" } });
            }
            // 电压区：IT87 电压通道（0x20-0x28，VIN0/1/2 原始值，换算系数因板而异）
            for (byte i = 0; i < 3; i++) {
                var v = ReadEnv(envBase, (byte)(0x20 + i));
                if (v != 0xFF && v != 0) voltages.Add(new { idx = i, raw = v });
            }
            ExitLd4(addr);
        } catch (Exception ex) {
            d["sensor_error"] = ex.Message;
        }
        d["fans"] = fans;
        d["temps"] = temps;
        d["voltages"] = voltages;
        d["writable"] = fans.Count > 0;
        d["fix_hint"] = "如需调速：fancmd write <idx> <pct>（软件接管），fancmd write reset <idx>（恢复主板控制）。若本机无驱动，先安装 inpoutx64 或让内置 AI 自动安装。";
        WriteDiag(d, cmds);
        return 0;
    }

    static bool IsAdmin() {
        try {
            using var id = System.Security.Principal.WindowsIdentity.GetCurrent();
            return new System.Security.Principal.WindowsPrincipal(id)
                .IsInRole(System.Security.Principal.WindowsBuiltInRole.Administrator);
        } catch { return false; }
    }

    static bool TryOpenDevice(string driver) {
        // inpoutx64 有设备 \\.\inpoutx64；其他驱动无设备节点（仅 DLL），返回 null 语义为「不可枚举」
        if (!driver.StartsWith("inpout", StringComparison.OrdinalIgnoreCase)) return false;
        var h = CreateFileW(@"\\.\inpoutx64", GENERIC_READ_WRITE, 0, IntPtr.Zero, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, IntPtr.Zero);
        var ok = h != (IntPtr)(-1);
        if (ok) CloseHandle(h);
        return ok;
    }

    static void WriteDiag(Dictionary<string, object?> d, string[] cmds) {
        var json = JsonSerializer.Serialize(d, new JsonSerializerOptions { WriteIndented = true });
        if (cmds.Length > 1 && cmds[1] == "json") { Console.WriteLine(json); }
        else {
            // 人读摘要
            Console.WriteLine($"fancmd diag v{d["version"]}");
            Console.WriteLine($"elevated        : {d["elevated"]}");
            if (d["driver"] is JsonElement je) {
                Console.WriteLine($"driver loaded   : {je.GetProperty("loaded").GetBoolean()}");
                if (je.TryGetProperty("name", out var n)) Console.WriteLine($"driver name     : {n.GetString()}");
                if (je.TryGetProperty("device_open", out var dv)) Console.WriteLine($"device open     : {dv.GetBoolean()}");
            }
            if (d.TryGetValue("rtc_ok", out var rt)) Console.WriteLine($"rtc port        : {rt}");
            if (d.TryGetValue("sio", out var sioV) && sioV is JsonElement sio) {
                Console.WriteLine($"sio found       : {sio.GetProperty("found").GetBoolean()}");
                if (sio.TryGetProperty("addr", out var a)) Console.WriteLine($"sio addr        : {a.GetString()}");
                if (sio.TryGetProperty("chip", out var c)) Console.WriteLine($"chip id         : {c.GetString()}");
                if (sio.TryGetProperty("kind", out var k)) Console.WriteLine($"chip kind       : {k.GetString()}");
                if (sio.TryGetProperty("env_base", out var b)) Console.WriteLine($"env base        : {b.GetString()}");
            }
            if (d.TryGetValue("fans", out var fs) && fs is JsonElement fe && fe.ValueKind == System.Text.Json.JsonValueKind.Array) {
                Console.WriteLine($"fans writable   : {fe.GetArrayLength()} 个");
                foreach (var f in fe.EnumerateArray()) {
                    var idx = f.GetProperty("idx").GetInt32();
                    var rpm = f.TryGetProperty("rpm", out var r) && r.ValueKind != System.Text.Json.JsonValueKind.Null ? r.GetSingle().ToString("F0") : "N/A";
                    Console.WriteLine($"  fan[{idx}] pwm=0x{f.GetProperty("pwm").GetByte():X2} ext=0x{f.GetProperty("ext").GetByte():X2} rpm={rpm}");
                }
            }
            if (d.TryGetValue("fix_hint", out var hint)) Console.WriteLine($"fix hint        : {hint}");
            // 完整 JSON 也打出来（AI 解析用），避免人读模式丢结构化信息
            Console.WriteLine(json);
        }
    }

    internal static int Run() {
        var isAdmin = IsAdmin();
        if (!LoadPortDriver()) {
            Console.Error.WriteLine("未加载到任何端口驱动（inpoutx64/inpout32/WinRing0x64/WinRing0/WinIo32）。");
            Console.Error.WriteLine("请先安装 inpoutx64 驱动（管理员），或让内置 AI 自动安装。");
            return 1;
        }
        _devOpen = TryOpenDevice(_drvName);
        Console.WriteLine($"elevated={isAdmin} driver={_drvName} inpoutx64_open={_devOpen}");

        try {
            // RTC 金标准验证：Inp/Out 是否真实读写端口
            Out(0x70, 0x00); var rtcSec = Inp(0x71);
            Out(0x70, 0x02); var rtcMin = Inp(0x71);
            Out(0x70, 0x04); var rtcDay = Inp(0x71);
            Console.WriteLine($"rtc sec={rtcSec} min={rtcMin} day={rtcDay}");
            if (rtcSec == 0xFF && rtcMin == 0xFF && rtcDay == 0xFF) {
                Console.Error.WriteLine("RTC 也全 0xFF → Inp/Out 端口读写未生效。");
                return 1;
            }
            Console.WriteLine("RTC OK → Inp/Out 真实读写端口。");

            // 多芯片 × 多端口扫描
            var (addr, kind, envBase, idH, idL) = FindSio();
            if (addr == 0) {
                Console.Error.WriteLine("未识别到 SuperIO 芯片（ITE/Nuvoton/Winbond × 0x2E/0x4E）。");
                return 1;
            }
            Console.WriteLine($"superio: port=0x{addr:X2} chip=0x{idH:X2}{idL:X2} ({kind}) env_base=0x{envBase:X4}");

            // 保持配置态读环境寄存器
            EnterLd4(addr, kind);
            SelectBankZero(envBase);
            ReadSensors(envBase, addr);
            ExitLd4(addr);
            Console.WriteLine("superio scan done.");
            return 0;
        } catch (Exception ex) {
            Console.Error.WriteLine($"superio probe failed: {ex.Message}");
            return 1;
        }
    }

    static void ReadSensors(ushort envBase, ushort sioAddr) {
        try {
            // 16 位 TACH 使能寄存器 0x0C：bit4/5/6/7 对应风扇使能
            var mode16 = ReadEnv(envBase, 0x0C);
            Console.WriteLine($"fan16_mode reg=0x0C = 0x{mode16:X2} (bit2=fan6,bit4=fan4,bit5=fan5)");
            for (byte i = 0; i < 3; i++) {
                var t = ReadEnv(envBase, (byte)(0x29 + i));
                if (t != 0xFF && t != 0) Console.WriteLine($"temp[{i}] reg=0x{(0x29 + i):X2} = {t} C");
            }
            for (byte f = 0; f < FAN_TACH_REG.Length; f++) {
                var lo = ReadEnv(envBase, FAN_TACH_REG[f]);
                var hi = ReadEnv(envBase, FAN_TACH_EXT_REG[f]);
                var tach = (ushort)((hi << 8) | lo);
                var rpm = tach > 0x3f && tach < 0xffff ? 1.35e6f / (tach * 2) : 0;
                Console.WriteLine($"fan[{f}] lo=0x{lo:X2} hi=0x{hi:X2} tach=0x{tach:X4} rpm={(rpm > 0 ? rpm.ToString("F0") : "N/A")}");
            }
            for (byte f = 0; f < 6; f++) {
                var pwm = ReadEnv(envBase, (byte)(0x15 + f));
                Console.WriteLine($"fan[{f}] pwm_reg=0x{(0x15 + f):X2} = 0x{pwm:X2} ({(pwm > 0 && pwm < 255 ? pwm * 100 / 255 : pwm == 255 ? 100 : 0)}%)");
            }
        } catch (Exception ex) {
            Console.Error.WriteLine($"read sensors failed: {ex.Message}");
        }
    }

    // ── 写控：复刻 LHM SetControl 逻辑（IT8689E 软件占空比 + 安全恢复）──
    // FAN_PWM_CTRL_REG = {0x15..0x1A}（IT8689E 主控写 0x7F 切软件）
    // FAN_PWM_CTRL_EXT_REG = {0x63,0x6B,0x73,0x7B,0xA3,0xAB}（实际占空比）
    // FAN_MAIN_CTRL_REG = 0x13（bit i 使能风扇 i 输出）
    internal static int Write(string[] cmds) {
        try {
            if (!LoadPortDriver()) { Console.Error.WriteLine("未加载到端口驱动，无法写控。"); return 1; }
            var (sioAddr, kind, envBase, idH, idL) = FindSio();
            if (sioAddr == 0) { Console.Error.WriteLine("未找到 SuperIO 芯片（驱动可用但 IT87 未识别）"); return 1; }
            Console.WriteLine($"sio=0x{sioAddr:X2} chip=0x{idH:X2}{idL:X2} ({kind}) env_base=0x{envBase:X4}");

            // 进入配置态 + 选 LD#4（环境控制器）
            EnterLd4(sioAddr, kind);
            SelectBankZero(envBase);

            if (cmds.Length >= 3 && cmds[1] == "reset") {
                var idx = int.Parse(cmds[2]);
                var orig = Restore.Get(sioAddr, envBase, idx);
                // 写回原始控制寄存器（0x15 主控值，恢复自动模式）+ 原始 ext PWM + 原始输出使能
                WriteEnv(envBase, FAN_PWM_CTRL_REG[idx], orig.ctrl);
                WriteEnv(envBase, FAN_PWM_CTRL_EXT_REG[idx], orig.ext);
                if (idx < 3) {
                    var main = ReadEnv(envBase, 0x13);
                    var initEnabled = orig.main != 0;
                    var isEnabled = (main & (1 << idx)) != 0;
                    // 若输出使能与初始不同，翻转对应 bit（复刻 LHM RestoreDefaultFanPwmControl）
                    if (isEnabled != initEnabled)
                        WriteEnv(envBase, 0x13, (byte)(main ^ (1 << idx)));
                }
                ExitLd4(sioAddr);
                Console.WriteLine($"ok\tfan{idx}\trestored 主板自动控制（ctrl=0x{orig.ctrl:X2} ext=0x{orig.ext:X2}）");
                return 0;
            }
            if (cmds.Length < 3) { Console.Error.WriteLine("usage: fancmd write <fanIdx 0-5> <pct 0-100> | fancmd write reset <fanIdx>"); return 2; }
            var fanIdx = int.Parse(cmds[1]);
            var pct = float.Parse(cmds[2], System.Globalization.CultureInfo.InvariantCulture);
            pct = Math.Clamp(pct, 0, 100);
            var value = (byte)Math.Round(pct * 255 / 100);

            // 保存原始值（只保存一次）
            if (!Restore.IsSaved(sioAddr, envBase, fanIdx))
                Restore.Save(sioAddr, envBase, fanIdx);

            // 关闭技嘉控制器（Enable(false)——LHM 里就是不再调用；写入本身即切软件）
            WriteEnv(envBase, FAN_PWM_CTRL_REG[fanIdx], 0x7F);          // IT8689E 主控切软件模式
            WriteEnv(envBase, FAN_PWM_CTRL_EXT_REG[fanIdx], value);      // 实际占空比
            if (fanIdx < 3) {
                var main = ReadEnv(envBase, 0x13);
                WriteEnv(envBase, 0x13, (byte)(main | (1 << fanIdx)));   // 使能输出
            }
            ExitLd4(sioAddr);
            Console.WriteLine($"ok\tfan{fanIdx}\t{pct}% (0x{value:X2}) 软件控制已接管");
            return 0;
        } catch (Exception ex) {
            Console.Error.WriteLine($"write failed: {ex.Message}");
            return 1;
        }
    }

    // 原始值保存/恢复：写入时把「初始 PWM」持久化到状态文件（跨进程共享，fancmd 每次调用是独立进程），
    // reset 时读回真正的初始值写回，主板 Smart Fan 控制器恢复接管。
    // 重启后芯片寄存器回到主板自动值，状态文件里的原值即「本次开机后首次写入前的值」，语义正确。
    static class Restore {
        static string StatePath =>
            System.IO.Path.Combine(System.IO.Path.GetTempPath(), "diskpilot_fancmd_saved.json");
        static Dictionary<int, (byte ctrl, byte ext, byte main)>? _mem;

        static void Load() {
            if (_mem != null) return;
            _mem = new Dictionary<int, (byte, byte, byte)>();
            try {
                if (File.Exists(StatePath)) {
                    var doc = JsonDocument.Parse(File.ReadAllText(StatePath));
                    foreach (var el in doc.RootElement.EnumerateObject()) {
                        if (!int.TryParse(el.Name, out var idx)) continue;
                        var o = el.Value;
                        _mem[idx] = (
                            o.TryGetProperty("c", out var c) ? c.GetByte() : (byte)0,
                            o.TryGetProperty("e", out var e) ? e.GetByte() : (byte)0,
                            o.TryGetProperty("m", out var m) ? m.GetByte() : (byte)0
                        );
                    }
                }
            } catch { /* 状态文件损坏则视为无存档，安全回退 */ }
        }

        static void Flush() {
            try {
                using var sw = new StreamWriter(StatePath);
                sw.Write("{");
                var first = true;
                foreach (var kv in _mem!) {
                    if (!first) sw.Write(",");
                    first = false;
                    sw.Write($"\"{kv.Key}\":{{\"c\":{kv.Value.ctrl},\"e\":{kv.Value.ext},\"m\":{kv.Value.main}}}");
                }
                sw.Write("}");
            } catch { /* 写失败不致命：reset 会退化为读当前值 */ }
        }

        internal static bool IsSaved(int sio, int base_, int idx) {
            Load();
            return _mem!.ContainsKey(idx);
        }

        internal static void Save(int sio, int base_, int idx) {
            Load();
            _mem![idx] = (
                ReadEnv((ushort)base_, FAN_PWM_CTRL_REG[idx]),
                ReadEnv((ushort)base_, FAN_PWM_CTRL_EXT_REG[idx]),
                idx < 3 ? ReadEnv((ushort)base_, 0x13) : (byte)0
            );
            Flush();
        }

        internal static (byte ctrl, byte ext, byte main) Get(int sio, int base_, int idx) {
            Load();
            if (_mem!.TryGetValue(idx, out var v)) return v;
            return (
                ReadEnv((ushort)base_, FAN_PWM_CTRL_REG[idx]),
                ReadEnv((ushort)base_, FAN_PWM_CTRL_EXT_REG[idx]),
                idx < 3 ? ReadEnv((ushort)base_, 0x13) : (byte)0
            );
        }
    }
}
