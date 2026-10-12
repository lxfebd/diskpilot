import { describe, expect, it } from 'vitest';
import {
  driveAliasesOf,
  driveLettersOf,
  gradeDiskHealth,
  pathDriveLetter,
  pathsOnRiskyDrives,
  renderDiskHealthText,
  resolveDriveLetter,
  resolveLetter,
  riskyDrives,
} from './diskHealth';

// 数据形状取自本机 hw_disk_health 的真实输出（Get-PhysicalDisk + Get-Partition 映射）。
const DISKS = [
  { friendly: 'NVME 4.0 1TB', health: 'Healthy', drive_letters: 'E' },
  { friendly: 'KXG70PNV2T04 NVMe KIOXIA 2048GB', health: 'Healthy', drive_letters: 'J' },
  { friendly: 'WDC WD20EZRX-19D8PB0', health: 'Healthy', drive_letters: 'I' },
  { friendly: 'FSLC MAL312000SA-T72', health: 'Healthy', drive_letters: 'F,G' },
  { friendly: 'WD_BLACK SN770 1TB', health: 'Healthy', drive_letters: 'C' },
];

// 分级回归：WMI HealthStatus 实测直出 Healthy / Warning / Unhealthy / Unknown，
// 而旧代码只认 Good/Caution——Healthy 落进 else，五块好盘全被画成红色「坏了」盾牌。
describe('gradeDiskHealth（硬盘健康原值分级）', () => {
  it('WMI 实测 Healthy → good（旧 bug：画成坏盘）', () => {
    expect(gradeDiskHealth('Healthy')).toBe('good');
    expect(gradeDiskHealth('healthy')).toBe('good');
    expect(gradeDiskHealth('OK')).toBe('good');
    expect(gradeDiskHealth('良好')).toBe('good');
  });

  it('Warning / Caution → caution', () => {
    expect(gradeDiskHealth('Warning')).toBe('caution');
    expect(gradeDiskHealth('Caution')).toBe('caution');
    expect(gradeDiskHealth('注意')).toBe('caution');
  });

  it('Unhealthy / NonRecoverable → bad', () => {
    expect(gradeDiskHealth('Unhealthy')).toBe('bad');
    expect(gradeDiskHealth('NonRecoverable')).toBe('bad');
    expect(gradeDiskHealth('不良')).toBe('bad');
  });

  it('Unknown / 空 / 没读到 → unknown，不当成坏盘', () => {
    expect(gradeDiskHealth('Unknown')).toBe('unknown');
    expect(gradeDiskHealth('')).toBe('unknown');
    expect(gradeDiskHealth(undefined)).toBe('unknown');
    expect(gradeDiskHealth(null)).toBe('unknown');
  });
});

describe('driveLettersOf（物理盘 → 盘符）', () => {
  it('多分区一块盘出多个盘符', () => {
    expect(driveLettersOf({ drive_letters: 'F,G' })).toEqual(['F', 'G']);
  });

  it('小写归一、去重、忽略空段', () => {
    expect(driveLettersOf({ drive_letters: 'c, C, ' })).toEqual(['C']);
  });

  it('映射不到时给空数组（不猜盘符）', () => {
    expect(driveLettersOf({})).toEqual([]);
    expect(driveLettersOf({ drive_letters: '' })).toEqual([]);
    expect(driveLettersOf(undefined)).toEqual([]);
  });
});

describe('pathDriveLetter（路径 → 盘符）', () => {
  it('认 X:\\、X:/ 与裸 X:', () => {
    expect(pathDriveLetter('C:\\Users\\me')).toBe('C');
    expect(pathDriveLetter('g:/media/movie.mkv')).toBe('G');
    expect(pathDriveLetter('D:')).toBe('D');
  });

  it('UNC / 相对路径 / 空串不认（没有本地物理盘）', () => {
    expect(pathDriveLetter('\\\\nas\\share\\f.txt')).toBe('');
    expect(pathDriveLetter('relative\\path')).toBe('');
    expect(pathDriveLetter('')).toBe('');
  });
});

describe('riskyDrives / pathsOnRiskyDrives（清理前的坏盘提醒）', () => {
  it('全 Healthy 时没有任何可疑盘', () => {
    expect(riskyDrives(DISKS)).toEqual([]);
    expect(pathsOnRiskyDrives(DISKS, ['C:\\Users\\x\\a.txt'])).toEqual([]);
  });

  it('unknown 不算可疑：没读到不等于坏', () => {
    const disks = [{ friendly: 'X', health: 'Unknown', drive_letters: 'D' }];
    expect(riskyDrives(disks)).toEqual([]);
  });

  it('只有路径落在可疑盘上才命中', () => {
    const disks = [
      { friendly: 'WD_BLACK SN770 1TB', health: 'Warning', drive_letters: 'C' },
      { friendly: 'Old Disk', health: 'Unhealthy', drive_letters: 'F,G' },
      { friendly: 'Good', health: 'Healthy', drive_letters: 'E' },
    ];
    const hit = pathsOnRiskyDrives(disks, ['C:\\Users\\me\\Downloads\\big.zip']);
    expect(hit).toHaveLength(1);
    expect(hit[0].friendly).toBe('WD_BLACK SN770 1TB');
    expect(hit[0].grade).toBe('caution');
    // 换到 E: 就不该被拦
    expect(pathsOnRiskyDrives(disks, ['E:\\Games\\data'])).toEqual([]);
    // 多盘符盘：G: 也算命中
    expect(pathsOnRiskyDrives(disks, ['G:\\Media\\movie.mkv'])[0].friendly).toBe('Old Disk');
  });

  it('可疑盘映射不到盘符时不命中（不凭型号名瞎拦）', () => {
    const disks = [{ friendly: 'USB Disk', health: 'Unhealthy', drive_letters: '' }];
    expect(pathsOnRiskyDrives(disks, ['C:\\Windows\\Temp\\x'])).toEqual([]);
  });

  it('UNC / 非盘符路径不命中', () => {
    const disks = [{ friendly: 'X', health: 'Warning', drive_letters: 'C' }];
    expect(pathsOnRiskyDrives(disks, ['\\\\nas\\share\\file.txt'])).toEqual([]);
  });
});

// 本机实测：D: 不在 Get-Partition 里，它是 `subst D: J:\D`——同一块 KIOXIA 上的映射盘。
// 不解析就会「D 没映射到任何物理盘」，对坏盘的清理提醒直接放过。
describe('映射盘（subst）解析', () => {
  const ALIASES = [{ letter: 'D', target: 'J:\\D' }];

  it('driveAliasesOf 归一化后端 substs，缺 target / 非单字母丢掉', () => {
    expect(
      driveAliasesOf({
        substs: [
          { letter: 'd', target: 'J:\\D' },
          { letter: 'D', target: 'K:\\other' },
          { letter: 'DD', target: 'J:\\x' },
          { letter: 'E', target: '' },
          null,
        ],
      }),
    ).toEqual([{ letter: 'D', target: 'J:\\D' }]);
    expect(driveAliasesOf({})).toEqual([]);
    expect(driveAliasesOf(null)).toEqual([]);
  });

  it('resolveLetter：映射盘解析成目标卷字母，普通盘原样', () => {
    expect(resolveLetter('D', ALIASES)).toBe('J');
    expect(resolveLetter('d', ALIASES)).toBe('J');
    expect(resolveLetter('C', ALIASES)).toBe('C');
    expect(resolveLetter('', ALIASES)).toBe('');
  });

  it('映射目标不是盘符路径时退回原字母（不猜）', () => {
    expect(resolveLetter('D', [{ letter: 'D', target: '\\\\nas\\share' }])).toBe('D');
    expect(resolveDriveLetter('D:\\Downloads\\a.zip', [{ letter: 'D', target: '\\\\nas\\share' }])).toBe('D');
  });

  it('resolveDriveLetter：D: 上的路径算到 J: 那块盘', () => {
    expect(resolveDriveLetter('D:\\Downloads\\a.zip', ALIASES)).toBe('J');
    expect(resolveDriveLetter('D:\\Downloads\\a.zip', [])).toBe('D');
  });

  it('可疑盘是 J 时，清理 D: 上的文件同样要被拦', () => {
    const disks = [{ friendly: 'KIOXIA 2TB', health: 'Unhealthy', drive_letters: 'J' }];
    const hit = pathsOnRiskyDrives(disks, ['D:\\Downloads\\big.zip'], ALIASES);
    expect(hit).toHaveLength(1);
    expect(hit[0].friendly).toBe('KIOXIA 2TB');
    // 不给别名表就漏掉——这正是修复前的行为。
    expect(pathsOnRiskyDrives(disks, ['D:\\Downloads\\big.zip'])).toEqual([]);
  });
});

describe('renderDiskHealthText（给 AI 的硬盘健康文本）', () => {
  const HEALTH = {
    disks: [
      {
        friendly: 'WD_BLACK SN770 1TB',
        media: 'SSD',
        bus: 'NVMe',
        size: 1000000000000,
        health: 'Healthy',
        drive_letters: 'C',
        power_on_hours: 2131,
        temperature_c: 41,
        wear_pct: 0,
        read_errors: 0,
        write_errors: 0,
      },
      {
        friendly: 'Mystery Disk',
        media: 'HDD',
        bus: 'SATA',
        size: 2000000000000,
        health: 'Unknown',
        drive_letters: '',
        power_on_hours: null,
        temperature_c: null,
        wear_pct: null,
        read_errors: null,
        write_errors: null,
      },
    ],
    reliability: true,
    substs: [{ letter: 'D', target: 'J:\\D' }],
  };

  it('分级 + 原值 + 盘符，模型照抄即可', () => {
    const text = renderDiskHealthText(HEALTH);
    expect(text).toContain('物理盘 2 块');
    expect(text).toContain('C: WD_BLACK SN770 1TB');
    expect(text).toContain('健康状态 Healthy（健康）');
    expect(text).toContain('通电 2131 小时');
  });

  it('Unknown 写成「未上报」并说明不等于坏，读不到的字段写「未读到」', () => {
    const text = renderDiskHealthText(HEALTH);
    expect(text).toContain('健康状态 Unknown（未上报：不等于坏，别当成坏盘）');
    expect(text).toContain('通电时间未读到');
    expect(text).toContain('磨损未读到');
    expect(text).toContain('读写错误未读到');
  });

  it('映射盘单独说明，不混进物理盘计数', () => {
    const text = renderDiskHealthText(HEALTH);
    expect(text).toContain('映射盘：D: → J:\\D');
    expect(text).toContain('别按两块盘算');
  });

  it('SMART 不可读时说明原因（非管理员权限常见）', () => {
    const text = renderDiskHealthText({ ...HEALTH, reliability: false });
    expect(text).toContain('SMART 可靠性计数器读不到');
  });

  it('CrystalDiskInfo 补充单独一段并标明口径不同', () => {
    const text = renderDiskHealthText({
      ...HEALTH,
      cdi_report: [{ device: 'WD_BLACK SN770', 健康状态: 'Good', 通电时间: '2131' }],
    });
    expect(text).toContain('CrystalDiskInfo 补充');
    expect(text).toContain('它用 Good/Caution');
    expect(text).toContain('健康状态 Good');
  });

  it('一块盘都没读到时直说，不给模型留猜测空间', () => {
    expect(renderDiskHealthText({ disks: [] })).toContain('不要凭猜测');
    expect(renderDiskHealthText(null)).toContain('不要凭猜测');
  });
});
