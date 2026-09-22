using System.Text;
using System.Text.Json;
using LibreHardwareMonitor.Hardware;

// fancmd — DiskPilot 传感器/风扇桥（LHM 内核，FanControl 同款；AIDA64 同类传感器集）。
// 用法:
//   fancmd list           枚举风扇与控制传感器（只读，精简）
//   fancmd sensors        全量传感器快照（温度/风扇/电压/功耗/频率/负载，JSON）
//   fancmd set FAN "<名|索引>" 35    写转速百分比（软件控制）
//   fancmd reset <名|索引>           恢复自动控制（关闭软件覆盖）
// 注意: 需要管理员权限才能开驱动（WinRing0/inpout）；无权限时 list/sensors 仍可用但 set 会失败。

var computer = new Computer {
    IsMotherboardEnabled = true,
    IsCpuEnabled = true,
    IsGpuEnabled = true,
    IsMemoryEnabled = true,
    IsStorageEnabled = true,
    IsControllerEnabled = true,
    IsNetworkEnabled = false,
};
computer.Open();
computer.Accept(new UpdateVisitor());

try {
    var cmds = Environment.GetCommandLineArgs().Skip(1).ToArray();
    // `--capture <tmp>`：admin 子进程专用，把 stdout/stderr 重定向到临时文件，
    // 供提权父进程读回打印。必须在命令分发之前解析并独占 `--capture` 参数槽。
    FileStream? capFs = null;
    if (cmds.Length >= 3 && cmds[0] == "--capture") {
        capFs = new FileStream(cmds[1], FileMode.Create, FileAccess.Write, FileShare.Read);
        Console.SetOut(new StreamWriter(capFs, new UTF8Encoding(false)) { AutoFlush = true });
        Console.SetError(Console.Out);
        cmds = cmds.Skip(2).ToArray();
    }
    if (cmds.Length == 0) { ShowList(computer); return; }
    if (cmds[0] is "list" or "ls") { ShowList(computer); return; }
    if (cmds[0] == "sensors") { ShowSensors(computer); return; }
    if (cmds[0] == "superio") { Environment.ExitCode = SuperIoProbe.Run(); return; }
    if (cmds[0] == "write") { Environment.ExitCode = SuperIoProbe.Write(cmds); return; }
    if (cmds[0] == "diag") { Environment.ExitCode = SuperIoProbe.Diag(cmds); return; }
    if (cmds[0] == "elevate") { Environment.ExitCode = Elevate.Run(cmds); return; }

    // 收集可写风扇（SensorType.Fan 且 Control 可用）
    var fans = new List<(string id, string name, float? value, IControl? ctrl, ISensor sensor, IHardware hw)>();
    foreach (var hw in computer.Hardware) {
        hw.Update();
        foreach (var s in hw.Sensors) {
            if (s.SensorType == SensorType.Fan) {
                var id = $"{hw.Identifier}/{s.Index}";
                fans.Add((id, $"{hw.Name} · {s.Name}", s.Value, s.Control, s, hw));
            }
        }
    }

    if (cmds[0] == "set" && cmds.Length >= 3) {
        var target = cmds[1]; var pct = float.Parse(cmds[2], System.Globalization.CultureInfo.InvariantCulture);
        pct = Math.Clamp(pct, 0, 100);
        var idx = -1; if (int.TryParse(target, out var n)) idx = n;
        var matches = fans.Where(f => idx >= 0 ? f.sensor.Index == idx || f.id.Contains($"/{idx}") : f.name.Contains(target, StringComparison.OrdinalIgnoreCase) || f.id.Contains(target, StringComparison.OrdinalIgnoreCase)).ToList();
        if (matches.Count == 0) { Console.Error.WriteLine($"no fan matched: {target}"); ShowList(computer); Environment.ExitCode = 2; return; }
        foreach (var m in matches) {
            var c = m.ctrl;
            if (c == null) { Console.Error.WriteLine($"fan has no control: {m.name} (read-only)"); continue; }
            try { c.SetSoftware(pct); Console.WriteLine($"ok\t{m.name}\t{pct}%"); }
            catch (Exception ex) { Console.Error.WriteLine($"set failed: {m.name}: {ex.Message}"); Environment.ExitCode = 1; }
        }
        return;
    }
    if (cmds[0] == "reset" && cmds.Length >= 2) {
        var target = cmds[1];
        var idx = -1; if (int.TryParse(target, out var n)) idx = n;
        var matches = fans.Where(f => idx >= 0 ? f.sensor.Index == idx || f.id.Contains($"/{idx}") : f.name.Contains(target, StringComparison.OrdinalIgnoreCase) || f.id.Contains(target, StringComparison.OrdinalIgnoreCase)).ToList();
        if (matches.Count == 0) { Console.Error.WriteLine($"no fan matched: {target}"); ShowList(computer); Environment.ExitCode = 2; return; }
        foreach (var m in matches) {
            var c = m.ctrl;
            if (c == null) continue;
            try { c.SetDefault(); Console.WriteLine($"ok\t{m.name}\tdefault"); }
            catch (Exception ex) { Console.Error.WriteLine($"reset failed: {m.name}: {ex.Message}"); Environment.ExitCode = 1; }
        }
        return;
    }
    Console.Error.WriteLine("usage: fancmd list | fancmd set <fan> <pct> | fancmd reset <fan>");
    Environment.ExitCode = 2;
} finally {
    computer.Close();
}

void ShowList(Computer computer) {
    var rows = new List<object>();
    foreach (var hw in computer.Hardware) {
        hw.Update();
        var fans = new List<object>();
        var temps = new List<object>();
        foreach (var s in hw.Sensors) {
            if (s.SensorType == SensorType.Fan) {
                fans.Add(new { idx = s.Index, name = s.Name,
                    rpm = s.Value.HasValue ? (float?)Math.Round(s.Value.Value) : (float?)null,
                    control = s.Control != null });
            } else if (s.SensorType == SensorType.Temperature && temps.Count < 6) {
                temps.Add(new { idx = s.Index, name = s.Name,
                    c = s.Value.HasValue ? (float?)Math.Round(s.Value.Value,1) : (float?)null });
            }
        }
        rows.Add(new { hardware = hw.Name, fans = fans, temps = temps });
    }
    Console.WriteLine(JsonSerializer.Serialize(rows, new JsonSerializerOptions { WriteIndented = true }));
}

/// <summary>全量传感器快照（JSON）：温度/风扇/电压/功耗/频率/负载，按硬件分组。
/// 对齐 AIDA64 的传感器覆盖（CPU/GPU/主板/内存/存储/风扇控制器），供内置 AI 读取。
/// 取两轮快照（间隔 1s）取最大值，绕开 LHM 首轮温度/电压常为 null 的坑。</summary>
void ShowSensors(Computer computer) {
    // 第一轮：开硬件 + 预热（温度传感器首轮常为 null）
    foreach (var hw in computer.Hardware) hw.Update();
    Thread.Sleep(1000);
    foreach (var hw in computer.Hardware) { hw.Update(); foreach (var sub in hw.SubHardware) sub.Update(); }

    var groups = new List<object>();
    foreach (var hw in computer.Hardware) {
        var sensorGroups = new Dictionary<string, List<object>>();
        void AddSensor(ISensor s) {
            var v = s.Value;
            var entry = new { idx = s.Index, name = s.Name, value = v.HasValue ? Math.Round(v.Value, 2) : (double?)null, min = s.Min.HasValue ? Math.Round(s.Min.Value, 2) : (double?)null, max = s.Max.HasValue ? Math.Round(s.Max.Value, 2) : (double?)null };
            var key = s.SensorType.ToString();
            if (!sensorGroups.TryGetValue(key, out var list)) { list = new List<object>(); sensorGroups[key] = list; }
            list.Add(entry);
        }
        foreach (var s in hw.Sensors) AddSensor(s);
        foreach (var sub in hw.SubHardware) foreach (var s in sub.Sensors) AddSensor(s);
        groups.Add(new { hardware = hw.Name, id = hw.Identifier.ToString(), groups = sensorGroups });
    }
    Console.WriteLine(JsonSerializer.Serialize(groups, new JsonSerializerOptions { WriteIndented = true }));
}

sealed class UpdateVisitor : IVisitor {
    public void VisitComputer(IComputer c) => c.Traverse(this);
    public void VisitHardware(IHardware h) { h.Update(); foreach (var s in h.SubHardware) s.Accept(this); }
    public void VisitSensor(ISensor s) { }
    public void VisitParameter(IParameter p) { }
}
