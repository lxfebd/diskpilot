import type { Node, Scaffold, UndoEntry, SteamInventory, WorkshopItem } from './types';

const GB = 1024 ** 3;
const MB = 1024 ** 2;

function leaf(name: string, path: string, size: number, files: number, scaffold_id: string | null = null): Node {
  return {
    name,
    path,
    is_dir: true,
    size,
    file_count: files,
    children: [],
    scaffold_id,
    top_extensions: [],
  };
}

// 真实文件节点：FileView 需要 is_dir=false 的叶子才能展示排序/搜索/扩展名统计。
// 真实扫描产出的树里文件节点就是这个形状，这里按同构补一批样例。
function file(name: string, path: string, size: number): Node {
  return {
    name,
    path,
    is_dir: false,
    size,
    file_count: 1,
    children: [],
    scaffold_id: null,
    top_extensions: [],
  };
}

export const MOCK_TREE: Node = {
  name: 'C:',
  path: 'C:\\',
  is_dir: true,
  size: 187.1 * GB,
  file_count: 805_990,
  scaffold_id: null,
  top_extensions: [
    { ext: '.dll', bytes: 48.6 * GB, count: 56_729 },
    { ext: '(none)', bytes: 37.5 * GB, count: 165_172 },
    { ext: '.exe', bytes: 16.6 * GB, count: 6_644 },
    { ext: '.db', bytes: 6.4 * GB, count: 1_473 },
  ],
  children: [
    {
      name: 'Users', path: 'C:\\Users', is_dir: true, size: 90 * GB, file_count: 480_730,
      scaffold_id: null, top_extensions: [],
      children: [
        {
          name: '90740', path: 'C:\\Users\\90740', is_dir: true, size: 90.3 * GB, file_count: 478_120,
          scaffold_id: null, top_extensions: [],
          children: [
            {
              name: 'AppData', path: 'C:\\Users\\90740\\AppData', is_dir: true, size: 71.1 * GB, file_count: 410_220,
              scaffold_id: null, top_extensions: [],
              children: [
                {
                  name: 'Local', path: 'C:\\Users\\90740\\AppData\\Local', is_dir: true, size: 54.2 * GB, file_count: 308_510,
                  scaffold_id: null, top_extensions: [],
                  children: [
                    // R205：Windows 使用痕迹（windows-privacy 命中）——Recent /
                    // 缩略图缓存 / INetCache 是真实磁盘上必然存在的隐私痕迹，
                    // 预览模式也给出可演示数据（清理页「隐私清理」不再恒「未检测到」）。
                    {
                      name: 'Microsoft', path: 'C:\\Users\\90740\\AppData\\Local\\Microsoft', is_dir: true,
                      size: 15.6 * GB, file_count: 95_120, scaffold_id: null, top_extensions: [],
                      children: [
                        {
                          name: 'Windows', path: 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows', is_dir: true,
                          size: 1.9 * GB, file_count: 6_720, scaffold_id: 'windows-privacy', top_extensions: [],
                          children: [
                            {
                              name: 'Explorer', path: 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\Explorer', is_dir: true,
                              size: 1.2 * GB, file_count: 3_100, scaffold_id: 'windows-privacy', top_extensions: [],
                              children: [
                                file('thumbcache_256.db', 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\Explorer\\thumbcache_256.db', 180 * 1024 * 1024),
                                file('thumbcache_1024.db', 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\Explorer\\thumbcache_1024.db', 420 * 1024 * 1024),
                                file('iconcache_32.db', 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\Explorer\\iconcache_32.db', 96 * 1024 * 1024),
                              ],
                            },
                            {
                              name: 'INetCache', path: 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\INetCache', is_dir: true,
                              size: 520 * MB, file_count: 2_610, scaffold_id: 'windows-privacy', top_extensions: [],
                              children: [
                                file('F0A1B2C3.dat', 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\INetCache\\IE\\F0A1B2C3.dat', 48 * 1024 * 1024),
                                file('D4E5F607.dat', 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\INetCache\\IE\\D4E5F607.dat', 96 * 1024 * 1024),
                              ],
                            },
                            // R228：WER 错误报告（crash-dumps scope 2）——%LOCALAPPDATA%\
                            // Microsoft\Windows\WER 真实常驻，崩溃转储脚本的第二个
                            // scope 行显示自己的真实字节（不再与 CrashDumps 共享总量）。
                            {
                              name: 'WER', path: 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\WER', is_dir: true,
                              size: 320 * MB, file_count: 1_480, scaffold_id: 'crash-dumps', top_extensions: [],
                              children: [
                                file('ReportQueue\\20261015_WER_Explorer.exe_cab', 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\WER\\ReportQueue\\20261015_WER_Explorer.exe_cab', 96 * 1024 * 1024),
                                file('ReportArchive\\20261001_WER_notepad.exe_cab', 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Windows\\WER\\ReportArchive\\20261001_WER_notepad.exe_cab', 64 * 1024 * 1024),
                              ],
                            },
                          ],
                        },
                      ],
                    },
                    // R205：Firefox 配置文件（browser-privacy 命中）——真实磁盘
                    // 常驻的浏览痕迹目录；与 Chrome/Edge 的 User Data（已归 browser-cache）
                    // 互补，让「浏览器使用痕迹」脚本在预览可演示。
                    // R207：同一 Firefox 树补 firefox-cache 命中——profiles 同级
                    // 的 cache2 / startupCache / shader-cache 归缓存脚本，Profiles
                    // 内只留浏览痕迹（visit 命中即不下钻，两者互不抢占）。
                    {
                      name: 'Mozilla', path: 'C:\\Users\\90740\\AppData\\Local\\Mozilla', is_dir: true,
                      size: 2.1 * GB, file_count: 8_600, scaffold_id: null, top_extensions: [],
                      children: [
                        {
                          name: 'Firefox', path: 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox', is_dir: true,
                          size: 2.1 * GB, file_count: 8_600, scaffold_id: null, top_extensions: [],
                          children: [
                            {
                              name: 'Profiles', path: 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\Profiles', is_dir: true,
                              size: 0.9 * GB, file_count: 6_190, scaffold_id: 'browser-privacy', top_extensions: [],
                              children: [
                                {
                                  name: 'abc12345.default-release', path: 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\Profiles\\abc12345.default-release', is_dir: true,
                                  size: 0.9 * GB, file_count: 6_180, scaffold_id: 'browser-privacy', top_extensions: [],
                                  children: [
                                    file('formhistory.sqlite', 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\Profiles\\abc12345.default-release\\formhistory.sqlite', 28 * 1024 * 1024),
                                    file('places.sqlite', 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\Profiles\\abc12345.default-release\\places.sqlite', 210 * 1024 * 1024),
                                  ],
                                },
                              ],
                            },
                            {
                              name: 'cache2', path: 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\cache2', is_dir: true,
                              size: 0.7 * GB, file_count: 1_420, scaffold_id: 'firefox-cache', top_extensions: [],
                              children: [
                                file('entries_A1.dat', 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\cache2\\entries_A1.dat', 96 * 1024 * 1024),
                                file('entries_B2.dat', 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\cache2\\entries_B2.dat', 48 * 1024 * 1024),
                              ],
                            },
                            {
                              name: 'shader-cache', path: 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\shader-cache', is_dir: true,
                              size: 0.5 * GB, file_count: 990, scaffold_id: 'firefox-cache', top_extensions: [],
                              children: [
                                file('shader_cache_0.bin', 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\shader-cache\\shader_cache_0.bin', 128 * 1024 * 1024),
                                file('shader_cache_1.bin', 'C:\\Users\\90740\\AppData\\Local\\Mozilla\\Firefox\\shader-cache\\shader_cache_1.bin', 64 * 1024 * 1024),
                              ],
                            },
                          ],
                        },
                      ],
                    },
                    {
                      name: 'Edge', path: 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Edge', is_dir: true,
                      size: 12.8 * GB, file_count: 41_320, scaffold_id: 'browser-cache',
                      top_extensions: [{ ext: '(none)', bytes: 6 * GB, count: 12_000 }],
                      children: [
                        leaf('User Data', 'C:\\Users\\90740\\AppData\\Local\\Microsoft\\Edge\\User Data', 12.5 * GB, 40_900, 'browser-cache'),
                      ],
                    },
                    {
                      name: 'Google', path: 'C:\\Users\\90740\\AppData\\Local\\Google', is_dir: true,
                      size: 6.4 * GB, file_count: 22_100, scaffold_id: 'browser-cache',
                      top_extensions: [], children: [
                        leaf('Chrome', 'C:\\Users\\90740\\AppData\\Local\\Google\\Chrome', 6.4 * GB, 22_100, 'browser-cache'),
                      ],
                    },
                    leaf('npm-cache', 'C:\\Users\\90740\\AppData\\Local\\npm-cache', 1.8 * GB, 8_400, 'dev-caches'),
                    leaf('pnpm', 'C:\\Users\\90740\\AppData\\Local\\pnpm', 4.2 * GB, 15_200, 'dev-caches'),
                    leaf('pip', 'C:\\Users\\90740\\AppData\\Local\\pip', 2.1 * GB, 5_300, 'dev-caches'),
                    leaf('JetBrains', 'C:\\Users\\90740\\AppData\\Local\\JetBrains', 3.6 * GB, 88_900, 'ide-caches'),
                    leaf('Docker', 'C:\\Users\\90740\\AppData\\Local\\Docker', 5.2 * GB, 1_200, 'docker-buildx'),
                    // R210：Unreal 派生数据缓存（engine-caches）——真实磁盘常驻
                    // 的 DDC 大头，预览演示游戏引擎缓存清理。
                    {
                      name: 'UnrealEngine', path: 'C:\\Users\\90740\\AppData\\Local\\UnrealEngine', is_dir: true,
                      size: 5.1 * GB, file_count: 18_600, scaffold_id: 'engine-caches', top_extensions: [],
                      children: [
                        {
                          name: 'Common', path: 'C:\\Users\\90740\\AppData\\Local\\UnrealEngine\\Common', is_dir: true,
                          size: 5.1 * GB, file_count: 18_590, scaffold_id: 'engine-caches', top_extensions: [],
                          children: [
                            {
                              name: 'DerivedDataCache', path: 'C:\\Users\\90740\\AppData\\Local\\UnrealEngine\\Common\\DerivedDataCache', is_dir: true,
                              size: 4.6 * GB, file_count: 14_200, scaffold_id: 'engine-caches', top_extensions: [],
                              children: [
                                file('ddc_0.udd', 'C:\\Users\\90740\\AppData\\Local\\UnrealEngine\\Common\\DerivedDataCache\\ddc_0.udd', 720 * 1024 * 1024),
                                file('ddc_1.udd', 'C:\\Users\\90740\\AppData\\Local\\UnrealEngine\\Common\\DerivedDataCache\\ddc_1.udd', 640 * 1024 * 1024),
                              ],
                            },
                          ],
                        },
                      ],
                    },
                    // R207：系统临时文件（system-temp）——每台 Windows 都有的
                    // %TEMP%，预览演示清理页不再恒「未检测到」。
                    {
                      name: 'Temp', path: 'C:\\Users\\90740\\AppData\\Local\\Temp', is_dir: true,
                      size: 2.8 * GB, file_count: 12_600, scaffold_id: 'system-temp', top_extensions: [],
                      children: [
                        file('tmpA8F1.tmp', 'C:\\Users\\90740\\AppData\\Local\\Temp\\tmpA8F1.tmp', 96 * 1024 * 1024),
                        file('tmpB2C3.tmp', 'C:\\Users\\90740\\AppData\\Local\\Temp\\tmpB2C3.tmp', 48 * 1024 * 1024),
                        file('installer.log', 'C:\\Users\\90740\\AppData\\Local\\Temp\\installer.log', 12 * 1024 * 1024),
                      ],
                    },
                    // R207：应用崩溃转储（crash-dumps）——CrashDumps / WER 真实
                    // 常驻的目录，预览演示崩溃转储清理。
                    {
                      name: 'CrashDumps', path: 'C:\\Users\\90740\\AppData\\Local\\CrashDumps', is_dir: true,
                      size: 0.9 * GB, file_count: 1_240, scaffold_id: 'crash-dumps', top_extensions: [],
                      children: [
                        file('notepad.exe.2026-10-12.dmp', 'C:\\Users\\90740\\AppData\\Local\\CrashDumps\\notepad.exe.2026-10-12.dmp', 64 * 1024 * 1024),
                        file('explorer.exe.2026-10-14.dmp', 'C:\\Users\\90740\\AppData\\Local\\CrashDumps\\explorer.exe.2026-10-14.dmp', 128 * 1024 * 1024),
                      ],
                    },
                    // R213：Microsoft Teams 2.x 新版——%LOCALAPPDATA%\Packages\
                    // MSTeams_8wekyb3d8bbwe 真实常驻（UWP 容器），与 Roaming 下
                    // 1.x 经典版双变体同屏演示（R210 的 detectVariants 修复点）。
                    {
                      name: 'Packages', path: 'C:\\Users\\90740\\AppData\\Local\\Packages', is_dir: true,
                      size: 3.2 * GB, file_count: 28_400, scaffold_id: null, top_extensions: [],
                      children: [
                        {
                          name: 'MSTeams_8wekyb3d8bbwe', path: 'C:\\Users\\90740\\AppData\\Local\\Packages\\MSTeams_8wekyb3d8bbwe', is_dir: true,
                          size: 3.2 * GB, file_count: 28_380, scaffold_id: 'teams', top_extensions: [],
                          children: [
                            {
                              name: 'LocalCache', path: 'C:\\Users\\90740\\AppData\\Local\\Packages\\MSTeams_8wekyb3d8bbwe\\LocalCache', is_dir: true,
                              size: 1.8 * GB, file_count: 12_400, scaffold_id: 'teams', top_extensions: [],
                              children: [
                                file('chrome_cache_001', 'C:\\Users\\90740\\AppData\\Local\\Packages\\MSTeams_8wekyb3d8bbwe\\LocalCache\\chrome_cache_001', 48 * 1024 * 1024),
                                file('blob_media_20261015.bin', 'C:\\Users\\90740\\AppData\\Local\\Packages\\MSTeams_8wekyb3d8bbwe\\LocalCache\\blob_media_20261015.bin', 32 * 1024 * 1024),
                              ],
                            },
                            {
                              name: 'TempState', path: 'C:\\Users\\90740\\AppData\\Local\\Packages\\MSTeams_8wekyb3d8bbwe\\TempState', is_dir: true,
                              size: 1.4 * GB, file_count: 15_900, scaffold_id: 'teams', top_extensions: [],
                              children: [
                                file('tmp_meeting_20261016.dat', 'C:\\Users\\90740\\AppData\\Local\\Packages\\MSTeams_8wekyb3d8bbwe\\TempState\\tmp_meeting_20261016.dat', 64 * 1024 * 1024),
                              ],
                            },
                          ],
                        },
                      ],
                    },
                  ],
                },
                {
                  name: 'Roaming', path: 'C:\\Users\\90740\\AppData\\Roaming', is_dir: true, size: 24.3 * GB, file_count: 137_120,
                  scaffold_id: null, top_extensions: [], children: [
                    // R209：腾讯 QQ（经典版）——%APPDATA%\Tencent\QQ 是真实
                    // 常驻目录，预览演示 QQ 接收文件/图片/缓存清理。
                    {
                      name: 'Tencent', path: 'C:\\Users\\90740\\AppData\\Roaming\\Tencent', is_dir: true,
                      size: 3.6 * GB, file_count: 14_500, scaffold_id: null, top_extensions: [],
                      children: [
                        {
                          name: 'QQ', path: 'C:\\Users\\90740\\AppData\\Roaming\\Tencent\\QQ', is_dir: true,
                          size: 3.6 * GB, file_count: 14_480, scaffold_id: 'qq-pc', top_extensions: [],
                          children: [
                            {
                              name: '123456789', path: 'C:\\Users\\90740\\AppData\\Roaming\\Tencent\\QQ\\123456789', is_dir: true,
                              size: 3.6 * GB, file_count: 14_470, scaffold_id: 'qq-pc', top_extensions: [],
                              children: [
                                {
                                  name: 'FileRecv', path: 'C:\\Users\\90740\\AppData\\Roaming\\Tencent\\QQ\\123456789\\FileRecv', is_dir: true,
                                  size: 2.1 * GB, file_count: 8_300, scaffold_id: 'qq-pc', top_extensions: [],
                                  children: [
                                    file('设计规范_v3.zip', 'C:\\Users\\90740\\AppData\\Roaming\\Tencent\\QQ\\123456789\\FileRecv\\设计规范_v3.zip', 480 * 1024 * 1024),
                                    file('产品原型_2026.pptx', 'C:\\Users\\90740\\AppData\\Roaming\\Tencent\\QQ\\123456789\\FileRecv\\产品原型_2026.pptx', 320 * 1024 * 1024),
                                  ],
                                },
                                {
                                  name: 'ImageRecv', path: 'C:\\Users\\90740\\AppData\\Roaming\\Tencent\\QQ\\123456789\\ImageRecv', is_dir: true,
                                  size: 1.1 * GB, file_count: 5_100, scaffold_id: 'qq-pc', top_extensions: [],
                                  children: [
                                    file('screenshot_2026-10-15.png', 'C:\\Users\\90740\\AppData\\Roaming\\Tencent\\QQ\\123456789\\ImageRecv\\screenshot_2026-10-15.png', 64 * 1024 * 1024),
                                  ],
                                },
                              ],
                            },
                          ],
                        },
                      ],
                    },
                    // R205：Windows Recent（最近打开文件列表，windows-privacy 命中）
                    {
                      name: 'Microsoft', path: 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft', is_dir: true,
                      size: 0.7 * GB, file_count: 2_400, scaffold_id: 'windows-privacy', top_extensions: [],
                      children: [
                        {
                          name: 'Windows', path: 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Windows', is_dir: true,
                          size: 0.7 * GB, file_count: 2_390, scaffold_id: 'windows-privacy', top_extensions: [],
                          children: [
                            {
                              name: 'Recent', path: 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Windows\\Recent', is_dir: true,
                              size: 0.7 * GB, file_count: 2_380, scaffold_id: 'windows-privacy', top_extensions: [],
                              children: [
                                file('report-final.docx.lnk', 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Windows\\Recent\\report-final.docx.lnk', 96 * 1024),
                                file('meeting-2026-09.mp4.lnk', 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Windows\\Recent\\meeting-2026-09.mp4.lnk', 128 * 1024),
                                file('installer_x64.exe.lnk', 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Windows\\Recent\\installer_x64.exe.lnk', 104 * 1024),
                              ],
                            },
                          ],
                        },
                      ],
                    },
                    // R211：OBS Studio——%APPDATA%\obs-studio 真实常驻（直播/
                    // 录屏用户高频），预览演示 OBS 崩溃转储/日志清理。
                    {
                      name: 'obs-studio', path: 'C:\\Users\\90740\\AppData\\Roaming\\obs-studio', is_dir: true,
                      size: 1.1 * GB, file_count: 1_890, scaffold_id: 'obs-cache', top_extensions: [],
                      children: [
                        {
                          name: 'crash-dumps', path: 'C:\\Users\\90740\\AppData\\Roaming\\obs-studio\\crash-dumps', is_dir: true,
                          size: 0.6 * GB, file_count: 480, scaffold_id: 'obs-cache', top_extensions: [],
                          children: [
                            file('crash_2026-10-14_08-32-15.log', 'C:\\Users\\90740\\AppData\\Roaming\\obs-studio\\crash-dumps\\crash_2026-10-14_08-32-15.log', 48 * 1024 * 1024),
                          ],
                        },
                        {
                          name: 'logs', path: 'C:\\Users\\90740\\AppData\\Roaming\\obs-studio\\logs', is_dir: true,
                          size: 0.5 * GB, file_count: 1_400, scaffold_id: 'obs-cache', top_extensions: [],
                          children: [
                            file('2026-10-15 21-07-12.txt', 'C:\\Users\\90740\\AppData\\Roaming\\obs-studio\\logs\\2026-10-15 21-07-12.txt', 16 * 1024 * 1024),
                            file('2026-10-13 19-44-50.txt', 'C:\\Users\\90740\\AppData\\Roaming\\obs-studio\\logs\\2026-10-13 19-44-50.txt', 12 * 1024 * 1024),
                          ],
                        },
                      ],
                    },
                    // R214：Discord——%APPDATA%\discord 真实常驻（Electron
                    // 缓存：Cache/GPUCache/blob_storage/Crashpad），预览演示
                    // Discord 缓存清理。
                    {
                      name: 'discord', path: 'C:\\Users\\90740\\AppData\\Roaming\\discord', is_dir: true,
                      size: 1.4 * GB, file_count: 4_520, scaffold_id: 'discord', top_extensions: [],
                      children: [
                        {
                          name: 'Cache', path: 'C:\\Users\\90740\\AppData\\Roaming\\discord\\Cache', is_dir: true,
                          size: 0.6 * GB, file_count: 1_930, scaffold_id: 'discord', top_extensions: [],
                          children: [
                            file('f_0001C2.dat', 'C:\\Users\\90740\\AppData\\Roaming\\discord\\Cache\\f_0001C2.dat', 28 * 1024 * 1024),
                            file('f_0001D4.dat', 'C:\\Users\\90740\\AppData\\Roaming\\discord\\Cache\\f_0001D4.dat', 22 * 1024 * 1024),
                          ],
                        },
                        {
                          name: 'GPUCache', path: 'C:\\Users\\90740\\AppData\\Roaming\\discord\\GPUCache', is_dir: true,
                          size: 0.3 * GB, file_count: 620, scaffold_id: 'discord', top_extensions: [],
                          children: [
                            file('data_0', 'C:\\Users\\90740\\AppData\\Roaming\\discord\\GPUCache\\data_0', 14 * 1024 * 1024),
                          ],
                        },
                        {
                          name: 'blob_storage', path: 'C:\\Users\\90740\\AppData\\Roaming\\discord\\blob_storage', is_dir: true,
                          size: 0.3 * GB, file_count: 1_270, scaffold_id: 'discord', top_extensions: [],
                          children: [
                            file('blob_20261015.bin', 'C:\\Users\\90740\\AppData\\Roaming\\discord\\blob_storage\\blob_20261015.bin', 18 * 1024 * 1024),
                          ],
                        },
                        {
                          name: 'Crashpad', path: 'C:\\Users\\90740\\AppData\\Roaming\\discord\\Crashpad', is_dir: true,
                          size: 0.2 * GB, file_count: 700, scaffold_id: 'discord', top_extensions: [],
                          children: [
                            file('reports_20261014.dmp', 'C:\\Users\\90740\\AppData\\Roaming\\discord\\Crashpad\\reports_20261014.dmp', 16 * 1024 * 1024),
                          ],
                        },
                      ],
                    },
                    // R215：Zoom——%APPDATA%\zoom.us 真实常驻（视频会议高频），
                    // 预览演示 Zoom 音视频缓存/遥测/日志清理。补上后 21 脚本
                    // 全命中（仅剩「安全软件套装」需杀软互斥不演示）。
                    {
                      name: 'zoom.us', path: 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us', is_dir: true,
                      size: 1.7 * GB, file_count: 6_810, scaffold_id: 'zoom', top_extensions: [],
                      children: [
                        {
                          name: 'data', path: 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\data', is_dir: true,
                          size: 1.1 * GB, file_count: 3_900, scaffold_id: 'zoom', top_extensions: [],
                          children: [
                            {
                              name: 'WebRTC', path: 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\data\\WebRTC', is_dir: true,
                              size: 0.6 * GB, file_count: 2_100, scaffold_id: 'zoom', top_extensions: [],
                              children: [
                                file('rtc_video_20261015.bin', 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\data\\WebRTC\\rtc_video_20261015.bin', 42 * 1024 * 1024),
                                file('rtc_audio_20261015.bin', 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\data\\WebRTC\\rtc_audio_20261015.bin', 28 * 1024 * 1024),
                              ],
                            },
                            {
                              name: 'Telemetry', path: 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\data\\Telemetry', is_dir: true,
                              size: 0.5 * GB, file_count: 1_800, scaffold_id: 'zoom', top_extensions: [],
                              children: [
                                file('telemetry_20261015.log', 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\data\\Telemetry\\telemetry_20261015.log', 12 * 1024 * 1024),
                              ],
                            },
                          ],
                        },
                        {
                          name: 'logs', path: 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\logs', is_dir: true,
                          size: 0.6 * GB, file_count: 2_900, scaffold_id: 'zoom', top_extensions: [],
                          children: [
                            file('zoom_20261015.log', 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\logs\\zoom_20261015.log', 16 * 1024 * 1024),
                            file('zoom_20261014.log', 'C:\\Users\\90740\\AppData\\Roaming\\zoom.us\\logs\\zoom_20261014.log', 14 * 1024 * 1024),
                          ],
                        },
                      ],
                    },
                    // R213：Microsoft Teams 1.x 经典版——%APPDATA%\Microsoft\Teams
                    // 真实常驻（Chromium 缓存），挂独立节点不抢 Microsoft 的
                    // windows-privacy 命中；与 Local 下 2.x 新版双变体同屏演示。
                    {
                      name: 'Teams', path: 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Teams', is_dir: true,
                      size: 0.9 * GB, file_count: 3_210, scaffold_id: 'teams', top_extensions: [],
                      children: [
                        {
                          name: 'Cache', path: 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Teams\\Cache', is_dir: true,
                          size: 0.4 * GB, file_count: 1_480, scaffold_id: 'teams', top_extensions: [],
                          children: [
                            file('f_0001A2.dat', 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Teams\\Cache\\f_0001A2.dat', 32 * 1024 * 1024),
                            file('f_0001B4.dat', 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Teams\\Cache\\f_0001B4.dat', 24 * 1024 * 1024),
                          ],
                        },
                        {
                          name: 'GPUCache', path: 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Teams\\GPUCache', is_dir: true,
                          size: 0.3 * GB, file_count: 620, scaffold_id: 'teams', top_extensions: [],
                          children: [
                            file('data_0', 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Teams\\GPUCache\\data_0', 16 * 1024 * 1024),
                          ],
                        },
                        {
                          name: 'media-stack', path: 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Teams\\media-stack', is_dir: true,
                          size: 0.2 * GB, file_count: 1_100, scaffold_id: 'teams', top_extensions: [],
                          children: [
                            file('audio_20261015.bin', 'C:\\Users\\90740\\AppData\\Roaming\\Microsoft\\Teams\\media-stack\\audio_20261015.bin', 24 * 1024 * 1024),
                          ],
                        },
                      ],
                    },
                  ],
                },
                leaf('LocalLow', 'C:\\Users\\90740\\AppData\\LocalLow', 2.1 * GB, 12_900, null),
              ],
            },
            {
              name: 'Documents', path: 'C:\\Users\\90740\\Documents', is_dir: true, size: 12.1 * GB, file_count: 18_400,
              scaffold_id: null, top_extensions: [],
              children: [
                {
                  name: 'WeChat Files', path: 'C:\\Users\\90740\\Documents\\WeChat Files', is_dir: true,
                  size: 11.4 * GB, file_count: 17_220, scaffold_id: 'wechat-pc',
                  top_extensions: [
                    { ext: '.dat', bytes: 6.8 * GB, count: 12_400 },
                    { ext: '.jpg', bytes: 2.1 * GB, count: 3_100 },
                    { ext: '.mp4', bytes: 1.6 * GB, count: 420 },
                  ],
                  children: [
                    {
                      name: 'wxid_gmsp9xjx12', path: 'C:\\Users\\90740\\Documents\\WeChat Files\\wxid_gmsp9xjx12',
                      is_dir: true, size: 10.7 * GB, file_count: 16_800, scaffold_id: 'wechat-pc', top_extensions: [],
                      children: [
                        file('demo.mp4', 'C:\\Users\\90740\\Documents\\WeChat Files\\wxid_gmsp9xjx12\\demo.mp4', 1.4 * GB),
                        file('photo_2026.jpg', 'C:\\Users\\90740\\Documents\\WeChat Files\\wxid_gmsp9xjx12\\photo_2026.jpg', 82 * 1024 * 1024),
                        file('screen_001.png', 'C:\\Users\\90740\\Documents\\WeChat Files\\wxid_gmsp9xjx12\\screen_001.png', 24 * 1024 * 1024),
                        file('voice_msg.amr', 'C:\\Users\\90740\\Documents\\WeChat Files\\wxid_gmsp9xjx12\\voice_msg.amr', 3.2 * 1024 * 1024),
                        file('notes.md', 'C:\\Users\\90740\\Documents\\WeChat Files\\wxid_gmsp9xjx12\\notes.md', 48 * 1024),
                        file('archive.zip', 'C:\\Users\\90740\\Documents\\WeChat Files\\wxid_gmsp9xjx12\\archive.zip', 320 * 1024 * 1024),
                      ],
                    },
                  ],
                },
              ],
            },
            leaf('.cargo', 'C:\\Users\\90740\\.cargo', 0.8 * GB, 12_400, 'dev-caches'),
            leaf('.cache/huggingface', 'C:\\Users\\90740\\.cache\\huggingface', 4.6 * GB, 320, 'huggingface-cache'),
            // R198：加 anaconda3（pkgs + envs）让清理页 Conda 脚本命中——
            // 命中后 isConda session 触发 listCondaEnvs mock，环境区可演示。
            {
              name: 'anaconda3', path: 'C:\\Users\\90740\\anaconda3', is_dir: true,
              size: 13.1 * GB, file_count: 42_300, scaffold_id: 'conda',
              top_extensions: [
                { ext: '.conda', bytes: 5.1 * GB, count: 1_240 },
                { ext: '.tar.bz2', bytes: 3.2 * GB, count: 860 },
              ],
              children: [
                {
                  name: 'pkgs', path: 'C:\\Users\\90740\\anaconda3\\pkgs', is_dir: true,
                  size: 4.2 * GB, file_count: 31_400, scaffold_id: 'conda',
                  top_extensions: [{ ext: '.conda', bytes: 3.1 * GB, count: 880 }],
                  children: [
                    leaf('cache', 'C:\\Users\\90740\\anaconda3\\pkgs\\cache', 0.9 * GB, 3_800, 'conda'),
                  ],
                },
                {
                  name: 'envs', path: 'C:\\Users\\90740\\anaconda3\\envs', is_dir: true,
                  size: 8.9 * GB, file_count: 10_900, scaffold_id: 'conda',
                  top_extensions: [], children: [
                    leaf('tf2', 'C:\\Users\\90740\\anaconda3\\envs\\tf2', 6.8 * GB, 6_400, 'conda'),
                    leaf('web', 'C:\\Users\\90740\\anaconda3\\envs\\web', 2.1 * GB, 4_500, 'conda'),
                  ],
                },
              ],
            },
          ],
        },
      ],
    },
    {
      name: 'Windows', path: 'C:\\Windows', is_dir: true, size: 36.2 * GB, file_count: 169_900,
      scaffold_id: null, top_extensions: [],
      children: [
        leaf('System32', 'C:\\Windows\\System32', 8.2 * GB, 32_400),
        leaf('WinSxS', 'C:\\Windows\\WinSxS', 7.7 * GB, 48_200),
        // R208：Windows 更新已下载缓存（windows-update）——真实 Windows
        // 必有 SoftwareDistribution\Download 目录的演示命中。
        {
          name: 'SoftwareDistribution', path: 'C:\\Windows\\SoftwareDistribution', is_dir: true,
          size: 2.4 * GB, file_count: 4_100, scaffold_id: 'windows-update', top_extensions: [],
          children: [
            {
              name: 'Download', path: 'C:\\Windows\\SoftwareDistribution\\Download', is_dir: true,
              size: 2.4 * GB, file_count: 4_090, scaffold_id: 'windows-update', top_extensions: [],
              children: [
                file('kb5048652_amd64.cab', 'C:\\Windows\\SoftwareDistribution\\Download\\kb5048652_amd64.cab', 680 * 1024 * 1024),
                file('kb5048652_arm64.cab', 'C:\\Windows\\SoftwareDistribution\\Download\\kb5048652_arm64.cab', 340 * 1024 * 1024),
                file('windows11.0-kb5049032-x64.msu', 'C:\\Windows\\SoftwareDistribution\\Download\\windows11.0-kb5049032-x64.msu', 420 * 1024 * 1024),
              ],
            },
          ],
        },
        // R228：系统崩溃转储（crash-dumps scope 3）——C:\Windows\Minidump
        // 真实常驻，系统崩溃转储 scope 行显示自己的真实字节。
        {
          name: 'Minidump', path: 'C:\\Windows\\Minidump', is_dir: true,
          size: 140 * MB, file_count: 42, scaffold_id: 'crash-dumps', top_extensions: [],
          children: [
            file('kernel.dmp', 'C:\\Windows\\Minidump\\kernel.dmp', 96 * 1024 * 1024),
            file('win32k.dmp', 'C:\\Windows\\Minidump\\win32k.dmp', 32 * 1024 * 1024),
          ],
        },
      ],
    },
    {
      name: 'Program Files', path: 'C:\\Program Files', is_dir: true, size: 24.5 * GB, file_count: 94_900,
      scaffold_id: null, top_extensions: [], children: [
        leaf('WindowsApps', 'C:\\Program Files\\WindowsApps', 9.5 * GB, 31_200),
        leaf('Steam', 'C:\\Program Files\\Steam', 4.8 * GB, 21_000, 'steam-shadercache'),
      ],
    },
    leaf('Program Files (x86)', 'C:\\Program Files (x86)', 17.4 * GB, 22_000),
    leaf('ProgramData', 'C:\\ProgramData', 9.7 * GB, 31_160),
    leaf('Recovery', 'C:\\Recovery', 3.7 * GB, 48),
    leaf('Eastmoney', 'C:\\Eastmoney', 1.1 * GB, 4_280),
    leaf('System Volume Information', 'C:\\System Volume Information', 0.96 * GB, 27),
    leaf('Microsoft VS Code', 'C:\\Microsoft VS Code', 0.43 * GB, 2_410),
  ],
};

// 浏览器/mock 预览用的 scaffold 清单——与 scaffolds/*.toml 逐份对齐（21 份）。
// 真实 Tauri 环境走后端 list_scaffolds（load_dir 只读 scaffolds/ 顶层 .toml）；
// 这里只服务 vite dev 预览。2026-09-08 整肃：删除 2026-05-05 已下架的 23 个
// legacy 幽灵项（dingtalk/feishu/slack/telegram/spotify/vscode/cursor/teams 旧版/
// battlenet/epicgames/brave/go-mod/gradle/maven/nuget/ollama/edge/chrome 旧版/
// npm/pnpm/pip/cargo/docker 旧版/jetbrains 旧版/steam 旧版/windows-temp 旧版/
// windows-old/recycle-bin/node-modules），替换为真实 TOML 的 scope 结构。
// 缺的补：firefox-cache / zoom / engine-caches / system-temp / crash-dumps /
// huggingface-cache / docker-buildx / ide-caches / steam-shadercache / conda。
export const SCAFFOLDS: Scaffold[] = [
  {
    id: 'wechat-pc', name: 'WeChat (PC)', risk: 'low',
    disclaimer: '清理微信（WeChat / 微信 4.x xwechat）的缓存、临时数据与接收的媒体文件。绝不触碰聊天记录数据库（db_storage、all_users/sqlite）、账号状态、收藏、朋友圈、加密物料。删除旧媒体后，超出保留期的历史图片/语音在聊天里会显示"已过期"。',
    detect: ['%USERPROFILE%/Documents/xwechat_files', '%USERPROFILE%/Documents/WeChat Files', '%APPDATA%/Tencent/xwechat', '%APPDATA%/Tencent/WeChat', '**/xwechat_files', '**/WeChat Files'],
    match: { name_contains: ['xwechat_files', 'WeChat Files'], must_have_child: ['all_users'] },
    scopes: [
      // 4.x 主线
      { id: 'chat-media-cache', label: '聊天媒体缓存', glob: '**/xwechat_files/wxid_*/cache/*/Message/**', mode: 'recycle', category: 'cache', variant: '4.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'web-resource-cache', label: '网页 / 小程序缓存', glob: '**/xwechat_files/wxid_*/cache/*/{HttpResource,WeAppIcon}/**', mode: 'recycle', category: 'cache', variant: '4.x', prompt: { kind: 'none' } },
      { id: 'sticker-cache', label: '表情缓存', glob: '**/xwechat_files/wxid_*/{cache/*/Emoticon,business/emoticon}/**', mode: 'recycle', category: 'cache', variant: '4.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'temp-files', label: '临时文件', glob: '**/xwechat_files/wxid_*/temp/**', mode: 'recycle', category: 'cache', variant: '4.x', prompt: { kind: 'none' } },
      { id: 'avatar-cache', label: '头像缓存', glob: '**/xwechat_files/all_users/head_imgs/**', mode: 'recycle', category: 'cache', variant: '4.x', prompt: { kind: 'none' } },
      { id: 'apm-records', label: '性能与统计记录', glob: '**/xwechat_files/wxid_*/apm_record/**', mode: 'recycle', category: 'cache', variant: '4.x', prompt: { kind: 'none' } },
      { id: 'received-files', label: '接收的文件', glob: '**/xwechat_files/wxid_*/msg/file/**', mode: 'recycle', category: 'media', variant: '4.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'received-videos', label: '接收的视频', glob: '**/xwechat_files/wxid_*/msg/video/**/*.{mp4,mov,m4v,3gp,mkv,webm,avi,m4s}', mode: 'recycle', category: 'media', variant: '4.x', prompt: { kind: 'days', default: 7, label: '保留最近多少天' } },
      { id: 'received-images', label: '接收的图片', glob: '**/xwechat_files/wxid_*/msg/video/**/*.{jpg,jpeg,png,gif,webp,bmp,heic,heif}', mode: 'recycle', category: 'media', variant: '4.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'voice-attachments', label: '语音 / 附件', glob: '**/xwechat_files/wxid_*/msg/attach/**', mode: 'recycle', category: 'media', variant: '4.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'chat-backups', label: '聊天备份', glob: '**/xwechat_files/Backup/wxid_*/**', mode: 'recycle', category: 'backup', variant: '4.x', prompt: { kind: 'confirm', label: '我确认要删除聊天备份' } },
      { id: 'app-logs-crashes', label: '应用日志与崩溃信息', glob: '%APPDATA%/Tencent/xwechat/{log,crashinfo}/**', mode: 'recycle', category: 'cache', variant: '4.x', prompt: { kind: 'none' } },
      { id: 'app-update-leftover', label: '安装包 / 远程配置缓存', glob: '%APPDATA%/Tencent/xwechat/{update,confsdk}/**', mode: 'recycle', category: 'cache', variant: '4.x', prompt: { kind: 'none' } },
      // 3.x legacy（4.x 机器上为空；UI 检测到 4.x 时整组隐藏）
      { id: 'image-cache-3x', label: '接收的图片', glob: '**/WeChat Files/wxid_*/{Image/Image,FileStorage/Image}/**', mode: 'recycle', category: 'media', variant: '3.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'video-cache-3x', label: '接收的视频', glob: '**/WeChat Files/wxid_*/{Video,FileStorage/Video}/**', mode: 'recycle', category: 'media', variant: '3.x', prompt: { kind: 'days', default: 7, label: '保留最近多少天' } },
      { id: 'file-cache-3x', label: '接收的文件', glob: '**/WeChat Files/wxid_*/{Files,FileStorage/File}/**', mode: 'recycle', category: 'media', variant: '3.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'voice-cache-3x', label: '语音消息', glob: '**/WeChat Files/wxid_*/FileStorage/{Voice2,Voice}/**', mode: 'recycle', category: 'media', variant: '3.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'msg-attach-3x', label: '消息缩略图缓存', glob: '**/WeChat Files/wxid_*/FileStorage/MsgAttach/**', mode: 'recycle', category: 'cache', variant: '3.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'sticker-cache-3x', label: '表情包缓存', glob: '**/WeChat Files/wxid_*/FileStorage/{Stickers,Emotion}/**', mode: 'recycle', category: 'cache', variant: '3.x', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'temp-files-3x', label: '临时文件', glob: '**/WeChat Files/wxid_*/FileStorage/Temp/**', mode: 'recycle', category: 'cache', variant: '3.x', prompt: { kind: 'none' } },
      { id: 'cache-misc-3x', label: '杂项缓存', glob: '**/WeChat Files/wxid_*/{Attachment,FileStorage/Cache}/**', mode: 'recycle', category: 'cache', variant: '3.x', prompt: { kind: 'none' } },
      { id: 'app-logs-crashes-3x', label: '应用日志与崩溃信息', glob: '%APPDATA%/Tencent/WeChat/{Log,Logs,CrashReport}/**', mode: 'recycle', category: 'cache', variant: '3.x', prompt: { kind: 'none' } },
      { id: 'app-update-leftover-3x', label: '安装包 / 更新缓存', glob: '%APPDATA%/Tencent/WeChat/Update/**', mode: 'recycle', category: 'cache', variant: '3.x', prompt: { kind: 'none' } },
    ],
  },
  {
    id: 'qq-pc', name: '腾讯 QQ（经典版）', risk: 'low',
    disclaimer: '只清经典版 9.x 的临时文件与接收的图片/视频/文件/表情（带保留期）。聊天数据库（nt_data 等）绝不触碰，QQNT 新版数据不覆盖。',
    detect: ['%APPDATA%/Tencent/QQ', '%APPDATA%/Tencent/QQNT', '**/Tencent/QQ', '**/Tencent/QQNT'],
    match: {},
    scopes: [
      { id: 'temp-cache', label: '临时文件', glob: '**/Tencent/QQ/{STemp,Temp}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' } },
      { id: 'received-files', label: '接收的文件', glob: '**/Tencent/QQ/*/{FileReceive,FileRecv}/**', mode: 'recycle', category: 'media', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'received-images', label: '接收的图片', glob: '**/Tencent/QQ/*/ImageRecv/**/*.{jpg,jpeg,png,gif,webp,bmp}', mode: 'recycle', category: 'media', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'received-videos', label: '接收的视频', glob: '**/Tencent/QQ/*/VideoRecv/**/*.{mp4,mov,m4v,3gp,mkv,webm,avi}', mode: 'recycle', category: 'media', prompt: { kind: 'days', default: 7, label: '保留最近多少天' } },
      { id: 'received-stickers', label: '接收的表情', glob: '**/Tencent/QQ/*/CustomFaceRecv/**/*.{gif,jpg,jpeg,png,apng,webp}', mode: 'recycle', category: 'media', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
    ],
  },
  {
    id: 'discord', name: 'Discord', risk: 'low',
    disclaimer: '只清 Electron 缓存（Cache / Code Cache / GPU 缓存 / Blob / 崩溃报告），重启自动重建。登录态与设置（Local Storage / IndexedDB / Network / Cookies / Login Data）绝不碰，清理后可能需重新登录部分网站。',
    detect: ['%APPDATA%/discord', '**/discord'],
    match: {},
    scopes: [
      { id: 'cache', label: 'Chromium 缓存', glob: '**/discord/Cache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' } },
      { id: 'code-cache', label: '代码缓存', glob: '**/discord/Code Cache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' } },
      { id: 'gpu-cache', label: 'GPU / WebGPU 缓存', glob: '**/discord/{GPUCache,DawnGraphiteCache,DawnWebGPUCache}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' } },
      { id: 'blob-storage', label: 'Blob 临时存储', glob: '**/discord/blob_storage/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' } },
      { id: 'crashpad', label: '崩溃报告', glob: '**/discord/Crashpad/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' } },
    ],
  },
  {
    id: 'teams', name: 'Microsoft Teams', risk: 'low',
    disclaimer: '清理 Teams（1.x 经典版 + 2.x 新版）的 Chromium 缓存/GPU 缓存/Blob 临时存储/崩溃报告/会议音视频临时文件，重启应用即可重建。绝不触碰登录态与账号（Local Storage/Session Storage/IndexedDB/Network/Cookies/Login Data）、desktop-config/app_settings.json、新版状态区（LocalState/RoamingState/Settings/SystemAppData）。',
    detect: ['%APPDATA%/Microsoft/Teams', '%LOCALAPPDATA%/Microsoft/Teams', '%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe', '**/Microsoft/Teams', '**/MSTeams_8wekyb3d8bbwe'],
    match: {},
    scopes: [
      // 1.x 经典版
      { id: 'cache', label: 'Chromium 缓存', glob: '%APPDATA%/Microsoft/Teams/Cache/**', mode: 'recycle', category: 'cache', variant: '1.x', prompt: { kind: 'none' } },
      { id: 'code-cache', label: '代码缓存', glob: '%APPDATA%/Microsoft/Teams/Code Cache/**', mode: 'recycle', category: 'cache', variant: '1.x', prompt: { kind: 'none' } },
      { id: 'gpu-cache', label: 'GPU / WebGPU 缓存', glob: '%APPDATA%/Microsoft/Teams/{GPUCache,DawnGraphiteCache,DawnWebGPUCache}/**', mode: 'recycle', category: 'cache', variant: '1.x', prompt: { kind: 'none' } },
      { id: 'blob-storage', label: 'Blob 临时存储', glob: '%APPDATA%/Microsoft/Teams/blob_storage/**', mode: 'recycle', category: 'cache', variant: '1.x', prompt: { kind: 'none' } },
      { id: 'crashpad', label: '崩溃报告', glob: '%APPDATA%/Microsoft/Teams/Crashpad/**', mode: 'recycle', category: 'cache', variant: '1.x', prompt: { kind: 'none' } },
      { id: 'media-stack', label: '会议音视频临时', glob: '%APPDATA%/Microsoft/Teams/media-stack/**', mode: 'recycle', category: 'media', variant: '1.x', prompt: { kind: 'none' } },
      { id: 'app-logs', label: '运行日志', glob: '%USERPROFILE%/AppData/{Roaming,Local}/Microsoft/Teams/{log,logs}/**', mode: 'recycle', category: 'cache', variant: '1.x', prompt: { kind: 'none' } },
      // 2.x 新版
      { id: 'new-app-cache', label: '新版 Application Cache', glob: '%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/AC/**', mode: 'recycle', category: 'cache', variant: '2.x', prompt: { kind: 'none' } },
      { id: 'new-local-cache', label: '新版 LocalCache', glob: '%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/LocalCache/**', mode: 'recycle', category: 'cache', variant: '2.x', prompt: { kind: 'none' } },
      { id: 'new-temp-state', label: '新版临时区', glob: '%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/TempState/**', mode: 'recycle', category: 'cache', variant: '2.x', prompt: { kind: 'none' } },
      { id: 'new-chromium-cache', label: '新版 Chromium 缓存', glob: '%LOCALAPPDATA%/Packages/MSTeams_8wekyb3d8bbwe/AppData/{Local,Roaming}/{Cache,Code Cache,GPUCache,ShaderCache,CodeCache,blob_storage,DawnGraphiteCache,DawnWebGPUCache,Crashpad}/**', mode: 'recycle', category: 'cache', variant: '2.x', prompt: { kind: 'none' } },
    ],
  },
  {
    id: 'zoom', name: 'Zoom', risk: 'low',
    disclaimer: '清理 Zoom 客户端的自建缓存：Chromium/GPU 缓存、WebRTC 音视频临时文件、媒体缓存、应用遥测与日志，删除后下次启动按需重建，不影响会议内容与账号。绝不触碰 Documents 下 Zoom 与 ZoomRecordings（会议录制，用户内容）、data 下 im 聊天/会议消息 DB 与账号配置、登录态与加密物料。',
    detect: ['%APPDATA%/zoom.us', '%APPDATA%/Zoom', '%LOCALAPPDATA%/Zoom', '%USERPROFILE%/Documents/Zoom', '**/zoom.us', '**/Zoom'],
    match: {},
    scopes: [
      { id: 'data-cache', label: '音视频缓存（WebRTC / 媒体缓存）', glob: '%APPDATA%/zoom.us/data/{WebRTC,media-cache}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' } },
      { id: 'telemetry', label: '应用遥测数据', glob: '%APPDATA%/zoom.us/data/Telemetry/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'logs', label: '客户端日志', glob: '%APPDATA%/zoom.us/logs/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'installer-downloads', label: '安装包与补丁归档', glob: '%APPDATA%/zoom.us/downloads/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
      { id: 'electron-cache', label: '浏览器缓存（Chromium / GPU）', glob: '%LOCALAPPDATA%/Zoom/{Cache,Code Cache,GPUCache,GrShaderCache,ShaderCache,DawnGraphiteCache,DawnWebGPUCache}', mode: 'recycle', category: 'cache', prompt: { kind: 'none' } },
      { id: 'electron-logs', label: '应用日志与崩溃报告', glob: '%LOCALAPPDATA%/Zoom/app/{logs,Crashpad}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '保留最近多少天' } },
    ],
  },
  {
    id: 'browser-cache', name: '浏览器缓存 Chrome / Edge', risk: 'low',
    disclaimer: '清理 Chrome / Edge 的联网缓存（Cache、Code Cache、GPU 缓存、着色器缓存等），删除后再次上网自动重建，不影响书签、密码、历史记录。绝不触碰 Local Storage、IndexedDB、Cookie、扩展程序数据、下载文件。',
    detect: ['%LOCALAPPDATA%/Google/Chrome/User Data', '%LOCALAPPDATA%/Microsoft/Edge/User Data'],
    match: { name_contains: ['User Data'], must_have_child: ['Cache'] },
    scopes: [
      { id: 'chrome-cache', label: 'Chrome 网页 / 媒体缓存', glob: '%LOCALAPPDATA%/Google/Chrome/User Data/**/{Cache,Code Cache,GPUCache,GrShaderCache,DawnGraphiteCache,DawnWebGPUCache,ShaderCache}', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'chrome-serviceworker-cache', label: 'Chrome Service Worker 缓存', glob: '%LOCALAPPDATA%/Google/Chrome/User Data/**/Service Worker/CacheStorage', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'edge-cache', label: 'Edge 网页 / 媒体缓存', glob: '%LOCALAPPDATA%/Microsoft/Edge/User Data/**/{Cache,Code Cache,GPUCache,GrShaderCache,DawnGraphiteCache,DawnWebGPUCache,ShaderCache}', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'edge-serviceworker-cache', label: 'Edge Service Worker 缓存', glob: '%LOCALAPPDATA%/Microsoft/Edge/User Data/**/Service Worker/CacheStorage', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'firefox-cache', name: 'Firefox 浏览器缓存', risk: 'low',
    disclaimer: '清理 Firefox 的联网缓存（cache2 / startupCache / shader-cache），删后自动重建，不影响书签、密码、历史、插件。绝不触碰 places.sqlite（书签/历史）、cookies.sqlite、logins.json、key4.db、formhistory.sqlite、storage/（网站数据）。',
    detect: ['%LOCALAPPDATA%/Mozilla/Firefox/Profiles', '%USERPROFILE%/.mozilla/firefox'],
    match: { name_contains: ['Profiles', 'firefox'] },
    scopes: [
      { id: 'firefox-network-cache', label: '网页缓存（cache2 / Cache）', glob: '%LOCALAPPDATA%/Mozilla/Firefox/Profiles/**/{cache2,Cache}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'firefox-startup-cache', label: '启动缓存（startupCache）', glob: '%LOCALAPPDATA%/Mozilla/Firefox/Profiles/**/startupCache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'firefox-shader-cache', label: '着色器缓存（shader-cache）', glob: '%LOCALAPPDATA%/Mozilla/Firefox/Profiles/**/shader-cache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'system-temp', name: '系统临时文件', risk: 'low',
    disclaimer: '清理 Windows 系统临时文件夹（%TEMP%）里的临时文件与缓存。只清理超过保留天数的旧文件（默认 7 天前），最近的文件可能正在被使用会自动跳过，不影响任何功能。',
    detect: ['%TEMP%', '%TMP%', '**/AppData/Local/Temp', '**/Temp'],
    match: {},
    scopes: [
      { id: 'temp-files', label: '临时文件（保留最近 7 天）', glob: '%TEMP%/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 7, label: '清理多少天前的临时文件' } },
    ],
  },
  {
    id: 'crash-dumps', name: '崩溃转储 / 错误报告', risk: 'low',
    disclaimer: '清理系统与应用崩溃留下的转储文件（.dmp）和 Windows 错误报告（WER）。这些是程序出问题时产生的诊断记录，删掉不影响任何功能。',
    detect: ['%LOCALAPPDATA%/CrashDumps', '%LOCALAPPDATA%/Microsoft/Windows/WER', 'C:/Windows/Minidump'],
    match: { name_contains: ['CrashDumps', 'Minidump', 'WER'] },
    scopes: [
      { id: 'crash-dumps-local', label: '应用崩溃转储（%LOCALAPPDATA%\\CrashDumps）', glob: '%LOCALAPPDATA%/CrashDumps/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的崩溃转储' } },
      { id: 'wer-reports', label: 'Windows 错误报告（WER）', glob: '%LOCALAPPDATA%/Microsoft/Windows/WER/{ReportQueue,ReportArchive}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的错误报告' } },
      { id: 'crash-dumps-system', label: '系统崩溃转储（C:\\Windows\\Minidump，需要管理员）', glob: 'C:/Windows/Minidump/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的系统崩溃转储' } },
    ],
  },
  {
    id: 'dev-caches', name: '开发缓存 npm / pip / 其它', risk: 'low',
    disclaimer: '清理开发工具的包缓存：npm、pnpm、Yarn、pip、Rust 的 Cargo 下载缓存，删掉不影响已安装的软件与项目，下次装包重下。绝不触碰 node_modules（项目依赖）、任何项目源码、pip 已安装的环境（site-packages）。',
    detect: ['%LOCALAPPDATA%/npm-cache', '%LOCALAPPDATA%/pnpm-cache', '%USERPROFILE%/.npm/_cacache', '%USERPROFILE%/.cargo/registry'],
    match: {},
    scopes: [
      { id: 'npm-cache', label: 'npm / pnpm / Yarn 包缓存', glob: '**/AppData/Local/{npm-cache,pnpm-cache}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的缓存' }, recycle_granularity: 'directory' },
      { id: 'yarn-cache', label: 'Yarn 包缓存', glob: '**/AppData/Local/Yarn/Cache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的缓存' }, recycle_granularity: 'directory' },
      { id: 'npm-cacache', label: 'npm 内容缓存（_cacache）', glob: '%USERPROFILE%/.npm/_cacache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的缓存' }, recycle_granularity: 'directory' },
      { id: 'pip-cache', label: 'pip 下载缓存', glob: '**/AppData/Local/pip/Cache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的缓存' }, recycle_granularity: 'directory' },
      { id: 'cargo-cache', label: 'Cargo 下载缓存（registry）', glob: '%USERPROFILE%/.cargo/registry/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的缓存' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'ide-caches', name: 'IDE 缓存 VSCode / Cursor / IntelliJ', risk: 'low',
    disclaimer: '清理 IDE 的本地缓存：VSCode / Cursor 的 Cache、Code Cache、GPUCache、CachedData，IntelliJ 系的 caches 与 index 目录，删后自动重建。绝不触碰扩展本体（extensions）、用户设置（settings.json）、项目源码、IntelliJ 项目 .idea。',
    detect: ['%APPDATA%/Code/Cache', '%APPDATA%/Cursor/Cache', '%USERPROFILE%/AppData/Local/JetBrains'],
    match: { name_contains: ['Code', 'Cursor', 'JetBrains'], must_have_child: ['Cache', 'CachedData'] },
    scopes: [
      { id: 'vscode-cache', label: 'VSCode 缓存', glob: '%APPDATA%/Code/{Cache,Code Cache,GPUCache,CachedData,logs}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'cursor-cache', label: 'Cursor 缓存', glob: '%APPDATA%/Cursor/{Cache,Code Cache,GPUCache,CachedData,logs}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'intellij-caches', label: 'IntelliJ 系缓存', glob: '%USERPROFILE%/AppData/Local/JetBrains/*/caches/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'intellij-index', label: 'IntelliJ 系索引', glob: '%USERPROFILE%/AppData/Local/JetBrains/*/index/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'obs-cache', name: 'OBS Studio 缓存', risk: 'low',
    disclaimer: '清理 OBS Studio 的运行缓存：崩溃转储与日志文件，删后按需重建。绝不触碰 obs-studio 下的 scene.json（场景配置）、scenes、profiles、plugin_config、basic 等配置目录，以及用户自己的录制视频。',
    detect: ['%APPDATA%/obs-studio'],
    match: { name_contains: ['obs-studio'] },
    scopes: [
      { id: 'obs-crash-dumps', label: 'OBS 崩溃转储', glob: '%APPDATA%/obs-studio/crash-dumps/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'obs-logs', label: 'OBS 日志文件', glob: '%APPDATA%/obs-studio/logs/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'docker-buildx', name: 'Docker Buildx 构建缓存', risk: 'low',
    disclaimer: '清理 Docker Buildx / BuildKit 的构建缓存（buildkitd 本地缓存：中间层 blobs、metadata 与缓存记录），删后下次构建自动重建。绝不触碰 Docker 镜像、容器、卷、compose 项目文件、Dockerfile 与项目源码。',
    detect: ['%USERPROFILE%/.docker/buildx', '%USERPROFILE%/.cache/buildkit'],
    match: { name_contains: ['buildkit', 'buildx'] },
    scopes: [
      { id: 'buildkit-cache', label: 'BuildKit 构建缓存', glob: '%USERPROFILE%/.cache/buildkit/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'docker-buildx-metadata', label: 'Docker Buildx 实例元数据', glob: '%USERPROFILE%/.docker/buildx/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'huggingface-cache', name: 'HuggingFace 模型缓存', risk: 'low',
    disclaimer: '清理 HuggingFace Hub 的下载缓存：模型、数据集与空间的缓存文件，删除后下次使用自动重新下载，不影响已下载内容之外的任何东西。',
    detect: ['%USERPROFILE%/.cache/huggingface'],
    match: { name_contains: ['huggingface'] },
    scopes: [
      { id: 'hf-hub-models', label: 'HuggingFace 模型缓存（hub）', glob: '%USERPROFILE%/.cache/huggingface/hub/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'hf-datasets-cache', label: 'HuggingFace 数据集缓存', glob: '%USERPROFILE%/.cache/huggingface/datasets/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'hf-spaces-cache', label: 'HuggingFace Spaces 缓存', glob: '%USERPROFILE%/.cache/huggingface/spaces/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'steam-shadercache', name: 'Steam 着色器缓存', risk: 'low',
    disclaimer: '清理 Steam 的着色器/管线缓存（steamapps/shadercache），删后下次启动游戏自动重建。绝不触碰 steamapps/common（游戏本体）、workshop（创意工坊）、userdata（存档与设置）、appmanifest*.acf。',
    detect: ['%ProgramFiles(x86)%/Steam/steamapps/shadercache', '%ProgramFiles%/Steam/steamapps/shadercache', '**/steamapps/shadercache'],
    match: { name_contains: ['shadercache'] },
    scopes: [
      { id: 'shader-cache', label: '游戏着色器缓存', glob: '{%ProgramFiles(x86)%/Steam,%ProgramFiles%/Steam,[A-Z]:/Steam*}/steamapps/shadercache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'engine-caches', name: '游戏引擎缓存（Unity / Unreal Engine / Godot）', risk: 'low',
    disclaimer: '清理 Unity / Unreal Engine / Godot 三家的引擎级缓存：Unity 包下载缓存与编辑器日志、Unity Hub 日志、Unreal 的派生数据缓存（DDC）、Godot 的编辑器缓存与导出缓存，删除后下次启动引擎自动重建。绝不触碰任何项目目录里的 Library、Assets、ProjectSettings、.git、.sln；.uproject 所在项目目录整体；Unreal Engine 安装目录与 Saved/Config；Godot 的 editor_settings.cfg / editor_layout.cfg 与 app_userdata。',
    detect: ['%LOCALAPPDATA%/Unity', '%APPDATA%/UnityHub', '%LOCALAPPDATA%/UnrealEngine', '%APPDATA%/Godot'],
    match: {},
    scopes: [
      { id: 'unity-package-cache', label: 'Unity 包下载缓存', glob: '%LOCALAPPDATA%/Unity/cache/**', mode: 'recycle', category: 'cache', variant: 'unity', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'unity-editor-logs', label: 'Unity 编辑器日志', glob: '%LOCALAPPDATA%/Unity/Editor/**', mode: 'recycle', category: 'cache', variant: 'unity', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'unity-hub-logs', label: 'Unity Hub 日志', glob: '{%LOCALAPPDATA%,%APPDATA%}/UnityHub/{logs,Logs}/**', mode: 'recycle', category: 'cache', variant: 'unity', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'unreal-derived-data-cache', label: 'Unreal 派生数据缓存（DDC）', glob: '%LOCALAPPDATA%/UnrealEngine/Common/DerivedDataCache/**', mode: 'recycle', category: 'cache', variant: 'unreal', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'unreal-dlc-partial', label: 'Unreal 派生数据缓存（DLC Partial）', glob: '%LOCALAPPDATA%/UnrealEngine/Common/DerivedDataCache_Partial/**', mode: 'recycle', category: 'cache', variant: 'unreal', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'godot-editor-cache', label: 'Godot 编辑器缓存', glob: '%APPDATA%/Godot/editor_cache/**', mode: 'recycle', category: 'cache', variant: 'godot', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
      { id: 'godot-export-cache', label: 'Godot 导出缓存', glob: '%APPDATA%/Godot/export_cache/**', mode: 'recycle', category: 'cache', variant: 'godot', prompt: { kind: 'none' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'conda', name: 'Conda / Anaconda / Miniconda', risk: 'medium',
    disclaimer: '清理 conda 包缓存（pkgs/）+ 长期未激活的 environments（envs/<name>/）。Base env、用户配置（.condarc）、~/.conda/environments.txt 永不触动；UI 上 base 灰显不可勾。走系统回收站可还原。',
    detect: ['%USERPROFILE%/anaconda3', '%USERPROFILE%/miniconda3', '%USERPROFILE%/miniforge3', '%USERPROFILE%/.conda'],
    match: { name_contains: ['anaconda3', 'miniconda3', 'miniforge3'], must_have_child: ['pkgs'] },
    scopes: [
      { id: 'tarballs', label: '下载的 tarball（pkgs/cache）', glob: '%USERPROFILE%/{anaconda3,miniconda3,miniforge3,.conda}/pkgs/cache', mode: 'recycle', category: 'cache', recycle_granularity: 'directory' },
      { id: 'unused-packages', label: '包缓存（按 mtime > 30 天）', glob: '%USERPROFILE%/{anaconda3,miniconda3,miniforge3,.conda}/pkgs/*', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30 }, recycle_granularity: 'directory' },
      { id: 'envs-stale', label: '未使用的 environments', glob: '%USERPROFILE%/{anaconda3,miniconda3,miniforge3,.conda}/envs/*', mode: 'recycle', category: 'envs', recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'security-suites', name: '安全软件套装（360 / 电脑管家 / 火绒）', risk: 'medium',
    disclaimer: '清理国产安全软件（360 家族 / 腾讯电脑管家 / 火绒）的 Chromium 内核浏览器缓存：缓存目录由浏览器自动重建，删除只会让网页图片/脚本下次重新下载，不影响书签、密码、历史记录、登录态。绝不触碰登录数据/Cookies/书签/历史、360 隔离区（被隔离文件可能牵涉用户资料）、任何 *.db 配置与账号验证文件。安全软件本体（360safe / QQPCMgr / Huorong 安装目录与规则库）只检测不清理，误删会破坏病毒库与规则，属「建议卸载而非清理」对象。清理走系统回收站，可还原。',
    detect: ['%LOCALAPPDATA%/360ChromeX/Chrome/User Data', '%LOCALAPPDATA%/360Chrome/Chrome/User Data', '%APPDATA%/360se6/User Data', '%PROGRAMFILES%/360/360safe', '%PROGRAMDATA%/Tencent/QQPCMgr', '%PROGRAMDATA%/Huorong', '**/360ChromeX/Chrome/User Data', '**/360Chrome/Chrome/User Data', '**/360se6/User Data', '**/360/360safe', '**/Tencent/QQPCMgr', '**/Huorong'],
    match: {},
    scopes: [
      { id: '360-browser-cache', label: '360 浏览器渲染缓存（图片 / 脚本）', glob: '**/360*/Chrome/User Data/*/{Cache,Code Cache,GPUCache,GrShaderCache,DawnCache,ShaderCache}/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 30, label: '清理多少天前的缓存' }, recycle_granularity: 'directory' },
      { id: '360se-cache', label: '360 安全浏览器（经典版）渲染缓存', glob: '**/360se6/User Data/*/{Cache,Code Cache,GPUCache,GrShaderCache}/**', mode: 'recycle', category: 'cache', variant: 'legacy', prompt: { kind: 'days', default: 30, label: '清理多少天前的缓存' }, recycle_granularity: 'directory' },
    ],
  },
  {
    id: 'windows-update', name: 'Windows 更新缓存', risk: 'medium',
    disclaimer: '清理 Windows 更新已下载的安装包缓存（SoftwareDistribution\\Download）与传递优化（Delivery Optimization）下载缓存。这些是更新安装完成后留下的旧副本，删掉不影响系统更新功能，之后的更新会按需重新下载。传递优化缓存默认会被系统自动清理，这里只在缓存明显偏大时手动回收。清理走系统回收站，可还原；系统目录需管理员权限，无权限的部分会自动跳过。',
    detect: ['C:/Windows/SoftwareDistribution', 'C:/Windows/ServiceProfiles/NetworkService/AppData/Local/Microsoft/Windows/DeliveryOptimization'],
    match: { name_contains: ['SoftwareDistribution', 'DeliveryOptimization'], must_have_child: [] },
    scopes: [
      { id: 'windows-update-download', label: 'Windows 更新安装包缓存（SoftwareDistribution\\Download）', glob: '%WINDIR%/SoftwareDistribution/Download/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 1, label: '清理多少天前的更新缓存（已安装的旧包）' }, recommended_selected: true },
      { id: 'delivery-optimization-cache', label: '传递优化下载缓存（Delivery Optimization）', glob: '%WINDIR%/ServiceProfiles/NetworkService/AppData/Local/Microsoft/Windows/DeliveryOptimization/Cache/**', mode: 'recycle', category: 'cache', prompt: { kind: 'days', default: 7, label: '清理多少天前的传递优化缓存' } },
    ],
  },
  {
    id: 'windows-privacy', name: 'Windows 活动与使用痕迹', risk: 'medium', tags: ['privacy'],
    disclaimer: '清理 Windows 留下的个人使用痕迹：最近打开的文件列表、缩略图缓存、IE/Edge 遗留的临时网络文件（INetCache）。这些是方便用途的索引与缓存，删掉不影响文件和程序本身，只是系统会重新生成。绝不触碰：浏览器书签、密码、登录信息、历史记录、Windows 账户凭据。缩略图删除后资源管理器会按需重建。清理走系统回收站，可还原。',
    detect: ['%APPDATA%/Microsoft/Windows/Recent', '%LOCALAPPDATA%/Microsoft/Windows/Explorer/thumbcache_*.db', '%LOCALAPPDATA%/Microsoft/Windows/INetCache'],
    match: { name_contains: ['Recent', 'thumbcache', 'INetCache'], must_have_child: [] },
    scopes: [
      { id: 'recent-items', label: '最近打开的文件列表（Recent / JumpList）', glob: '%APPDATA%/Microsoft/Windows/Recent/**', mode: 'recycle', category: 'privacy', prompt: { kind: 'none' } },
      { id: 'thumbcache', label: '资源管理器缩略图缓存（thumbcache）', glob: '%LOCALAPPDATA%/Microsoft/Windows/Explorer/thumbcache_*.db', mode: 'recycle', category: 'privacy', prompt: { kind: 'none' } },
      { id: 'inet-cache', label: 'IE / Edge 遗留临时网络文件（INetCache）', glob: '%LOCALAPPDATA%/Microsoft/Windows/INetCache/**', mode: 'recycle', category: 'privacy', prompt: { kind: 'days', default: 30, label: '清理多少天前的临时网络文件' } },
    ],
  },
  {
    id: 'browser-privacy', name: '浏览器使用痕迹（历史 / 表单）', risk: 'medium', tags: ['privacy'],
    disclaimer: '清理 Chrome / Edge 的浏览历史与 Firefox 的表单填写记录。这些记录会在这三个浏览器彻底关闭后才会完整写入磁盘，清理前请完全退出浏览器。绝不触碰：书签、密码（Login Data）、Cookie、会话登录状态、扩展程序数据、Local Storage、IndexedDB——Cookie 与登录数据属于项目安全红线，任何清理都不会触及。Firefox 浏览历史与书签同库（places.sqlite），为安全起见同样不清理。历史记录删除后不可在浏览器内找回。清理走系统回收站，可还原。',
    detect: ['%LOCALAPPDATA%/Google/Chrome/User Data/Default/History', '%LOCALAPPDATA%/Microsoft/Edge/User Data/Default/History', '%APPDATA%/Mozilla/Firefox/Profiles'],
    match: { name_contains: ['User Data', 'Firefox'], must_have_child: [] },
    scopes: [
      { id: 'chromium-history', label: 'Chrome / Edge 浏览历史', glob: '{%LOCALAPPDATA%/Google/Chrome/User Data,%LOCALAPPDATA%/Microsoft/Edge/User Data}/**/{History,History-journal,History-journal-ms}*', mode: 'recycle', category: 'privacy', prompt: { kind: 'none' }, required_stopped_processes: ['chrome.exe', 'msedge.exe'] },
      { id: 'firefox-formhistory', label: 'Firefox 表单填写记录', glob: '%APPDATA%/Mozilla/Firefox/Profiles/*/formhistory.sqlite*', mode: 'recycle', category: 'privacy', prompt: { kind: 'none' }, required_stopped_processes: ['firefox.exe'] },
    ],
  },
];

export async function scan(_path: string): Promise<Node> {
  await wait(800);
  // R225：克隆前先剪已回收路径——「清完重扫」不再复活已删目录。
  return pruneRecycledFrom(structuredClone(MOCK_TREE));
}

/** R182：预览模式的磁盘清单。真实桌面端从系统读盘（list_drives），浏览器
 *  里没有真盘——返回与 MOCK_TREE 同源的一块 C 盘（187.1GB 总量），让总览的
 *  磁盘详情、清理建议、空间趋势、救援横幅在演示时不再是「未检测到磁盘」。 */
export async function listDrives(): Promise<
  { total_bytes: number; used_bytes: number; free_bytes: number; path: string; mapped_to: string }[]
> {
  await wait(30);
  const total = 187.1 * GB;
  // R231：used 随会话释放台账下压——清完再扫描，工作台磁盘卡与空间趋势
  // 曲线一起下降（同一块 C 盘同一口径，不会「曲线回落了磁盘卡还满着」）。
  const used = Math.max(0, 148.1 * GB - sessionReclaimedBytes);
  return [{ total_bytes: total, used_bytes: used, free_bytes: total - used, path: 'C:\\', mapped_to: '' }];
}

/** R183：预览模式的空间历史——总览「磁盘空间趋势」卡的演示数据。
 *  与 listDrives 同一块 C 盘：近 14 天每日一采样，已用空间缓慢爬升（含
 *  R181/R182 演示清理时释放的一截回落，趋势卡能看到「清完下降」的增量）。
 *  采样 > 5 点 → 趋势卡判定 rich（可信），增量与耗尽预测都能展示。 */
/** R231：本次会话（页面刷新即清零）真删累计释放字节——getSpaceHistory
 *  今天末点用它下压，演示里「刚清完 276MB，空间趋势卡今天回落」与清理
 *  页/历史页同源。与 UNDO_LEDGER 同生命周期：真删累加、dry-run 不动。
 *  R233：per-source 入账台账——还原时只减回该 source 实际入账过的部分
 *  （种子隔离条目从没入账，还原不回涨；避免 clamp 到 0 时把别的清理
 *  释放也凭空回涨）。 */
let sessionReclaimedBytes = 0;
const reclaimedBySource = new Map<string, number>();

/** R231：测试专用——复位会话释放台账（各 describe 互不污染）。 */
export function _mockResetReclaimed(): void {
  sessionReclaimedBytes = 0;
  reclaimedBySource.clear();
}

export async function getSpaceHistory(): Promise<
  Record<string, { root: string; total_bytes: number; used_bytes: number; at: number }[]>
> {
  await wait(50);
  const total = 187.1 * GB;
  const DAY = 86_400;
  const now = Math.floor(Date.now() / 1000);
  // 14 天前 ~ 今天，每日 18:00 采样；今天用「刚扫描」的当前值（与 listDrives 一致）。
  const pts: { root: string; total_bytes: number; used_bytes: number; at: number }[] = [];
  const base = 143.6 * GB;
  for (let i = 13; i >= 0; i--) {
    const dayStart = now - (now % DAY) - i * DAY;
    const at = dayStart + 18 * 3600;
    // 缓慢爬升：约 0.4GB/天；第 4 天有一次演示清理回落（-2.0GB，模拟真实场景）。
    const growth = i === 0 ? 148.1 * GB : base + (13 - i) * 0.42 * GB - (i === 4 ? 2.0 * GB : 0);
    pts.push({ root: 'C:\\', total_bytes: total, used_bytes: Math.round(growth), at: at > now ? now : at });
  }
  // R231：今天末点 = 刚扫描值 − 本次会话已释放字节——清理后曲线今天回落
  // （真实后端每次清理记账并反映在后续采样；mock 用会话台账模拟同源口径）。
  const today = 148.1 * GB - sessionReclaimedBytes;
  pts[pts.length - 1] = { root: 'C:\\', total_bytes: total, used_bytes: Math.max(0, Math.round(today)), at: now };
  return { 'C:\\': pts };
}

// —— R184：预览模式的启动项演示数据 ——
// 真实桌面端从注册表/启动文件夹枚举（list_startup_items）；浏览器里没有系统
// 可读，给一批覆盖四个来源（HKCU/HKLM/用户夹/机器夹）的样例启动项，让设置页
// 「系统工具 → 启动项」的搜索、来源分组、启停/删除两步确认都能点可看。
// 只读枚举 mock；启停/删除走 tauriOnly 拒绝（那是有真实系统副作用的写操作，
// 浏览器里不演示假写成功）。

export interface MockStartupItem {
  id: string;
  name: string;
  command: string;
  location: string;
  enabled: boolean;
  target: string;
}

const MOCK_STARTUP: MockStartupItem[] = [
  { id: 'hkcu-qq', name: '腾讯QQ', command: '"C:\\Program Files (x86)\\Tencent\\QQ\\QQ.exe" -autostart', location: 'registry_hkcu', enabled: true, target: 'C:\\Program Files (x86)\\Tencent\\QQ\\QQ.exe' },
  { id: 'hkcu-wx', name: '微信', command: '"C:\\Program Files\\Tencent\\WeChat\\WeChat.exe" -autostart', location: 'registry_hkcu', enabled: true, target: 'C:\\Program Files\\Tencent\\WeChat\\WeChat.exe' },
  { id: 'hkcu-obs', name: 'OBS Studio', command: '"C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe" --minimize-to-tray', location: 'registry_hkcu', enabled: false, target: 'C:\\Program Files\\obs-studio\\bin\\64bit\\obs64.exe' },
  { id: 'hkcu-ts', name: '腾讯会议', command: '"C:\\Program Files (x86)\\Tencent\\wemeet\\wemeetapp.exe" --autostart', location: 'registry_hkcu', enabled: false, target: 'C:\\Program Files (x86)\\Tencent\\wemeet\\wemeetapp.exe' },
  { id: 'user-aria', name: 'aria2 下载器', command: 'D:\\Tools\\aria2\\aria2c.exe --enable-rpc', location: 'folder_user', enabled: true, target: 'D:\\Tools\\aria2\\aria2c.exe' },
  { id: 'user-utools', name: 'uTools 启动器', command: '"C:\\Users\\90740\\AppData\\Local\\Programs\\utools\\utools.exe" --autostart', location: 'folder_user', enabled: true, target: 'C:\\Users\\90740\\AppData\\Local\\Programs\\utools\\utools.exe' },
  { id: 'hklm-nv', name: 'NVIDIA 控制面板', command: '"C:\\Windows\\System32\\nvcpl.dll" RunDLL32', location: 'registry_hklm', enabled: true, target: 'C:\\Windows\\System32\\nvcpl.dll' },
  { id: 'machine-hp', name: 'HP 打印助手', command: '"C:\\Program Files\\HP\\HP Print Solutions\\HPPrint.exe" /minimized', location: 'folder_machine', enabled: false, target: 'C:\\Program Files\\HP\\HP Print Solutions\\HPPrint.exe' },
];

// —— R198：预览模式的 conda 环境演示数据 ——
// 真实桌面端 list_conda_envs 枚举 conda 安装下的 base + user 环境（只读，
// 含大小与最后活跃时间）；浏览器里没有 conda 可读，给 base + 2 个 user
// 环境（1 个 stale 90d 默认勾选），让清理页「Conda / Anaconda」脚本的
// 环境区能演示 base 灰显不可勾 + 用户环境勾选，而不是空列表。

export interface MockCondaEnv {
  name: string;
  path: string;
  size_bytes: number;
  last_active_ts: number | null;
  is_base: boolean;
  default_checked: boolean;
}

export async function listCondaEnvs(): Promise<MockCondaEnv[]> {
  await wait(60);
  const now = Math.floor(Date.now() / 1000);
  // R238：与回收台账同源——envs-stale 等 scope 真删环境目录后（executeScope
  // 写 _mockRecyclePath），删除完成时清理页刷新本列表不再返回已删环境
  // （真实桌面端 conda env 删了枚举就没了）；base 不受影响（永不被清）。
  const recycled = recycledPaths.map((p) => p.replace(/[\\/]+$/, '').toLowerCase());
  const isRecycled = (path: string) => {
    const low = path.replace(/[\\/]+$/, '').toLowerCase();
    return recycled.some((r) => low === r || low.startsWith(r + '\\') || low.startsWith(r + '/'));
  };
  const all: MockCondaEnv[] = [
    { name: 'base', path: 'C:\\Users\\90740\\anaconda3', size_bytes: 4.2 * GB, last_active_ts: now - 2 * DAY, is_base: true, default_checked: false },
    { name: 'tf2', path: 'C:\\Users\\90740\\anaconda3\\envs\\tf2', size_bytes: 6.8 * GB, last_active_ts: now - 120 * DAY, is_base: false, default_checked: true },
    { name: 'web', path: 'C:\\Users\\90740\\anaconda3\\envs\\web', size_bytes: 2.1 * GB, last_active_ts: now - 15 * DAY, is_base: false, default_checked: false },
  ];
  return all.filter((e) => e.is_base || !isRecycled(e.path));
}

// 模块级可变启动项状态（R197）：浏览器演示中「禁用/启用/删除」写操作
// 作用于这份内存态（同 R192 reminderCfg 模式），列表刷新后真实反映
// 切换/删除效果——不再「假成功」：toast 显示成功却列表纹丝不动。
// 真实桌面端 set/remove 会写注册表/启动文件夹（.disabled 备份 / 回收站），
// 浏览器只在内存态演示；删除项在本会话内不再出现（刷新页面即重置）。
let startupItems: MockStartupItem[] = MOCK_STARTUP.map((it) => ({ ...it }));

/** 只读枚举：返回当前启动项副本（调用方改动不污染模块级数组）。 */
export async function listStartupItems(): Promise<MockStartupItem[]> {
  await wait(60);
  return startupItems.map((it) => ({ ...it }));
}

/** 启停（写）：内存态翻转 enabled。真实桌面端 = 值/文件名加 .disabled 备份。 */
export async function setStartupItem(id: string, enable: boolean): Promise<void> {
  await wait(50);
  const it = startupItems.find((i) => i.id === id);
  if (it) it.enabled = enable;
}

/** 删除（写）：从内存态移除。真实桌面端 = 注册表值先写 .reg 备份 / 文件夹项进回收站。 */
export async function removeStartupItem(id: string): Promise<string> {
  await wait(50);
  const it = startupItems.find((i) => i.id === id);
  if (!it) return '';
  startupItems = startupItems.filter((i) => i.id !== id);
  // 演示备份路径：注册表项给 .reg 备份，文件夹项走回收站（空 = 回收站接住）。
  return it.location.startsWith('registry')
    ? `${it.name}.reg 备份（演示）`
    : '';
}

// —— R185：预览模式的已装驱动演示数据 ——
// 真实桌面端用 CIM 枚举（list_installed_drivers，秒回只读）；浏览器里没有系统
// 可读，给一批覆盖常见类别的样例驱动，让「系统工具 → 驱动检查」的已装列表、
// 类别筛选（R166）可点可看。只读 mock；`check_driver_updates` 联网查询走
// tauriOnly 拒绝（那是授权联网命令，浏览器不演示假查询）。

export interface MockInstalledDriver {
  name: string;
  provider: string;
  version: string;
  date: string;
  class: string;
}

const MOCK_DRIVERS: MockInstalledDriver[] = [
  { name: 'Intel(R) Iris(R) Xe Graphics', provider: 'Intel Corporation', version: '31.0.101.5186', date: '2026-06-18', class: 'display' },
  { name: 'Realtek(R) Audio', provider: 'Realtek Semiconductor Corp.', version: '6.0.9717.1', date: '2026-05-12', class: 'sound' },
  { name: 'Intel(R) Wi-Fi 6E AX211 160MHz', provider: 'Intel Corporation', version: '23.30.0.7', date: '2026-07-02', class: 'network' },
  { name: 'Realtek PCIe GbE Family Controller', provider: 'Realtek Semiconductor Corp.', version: '10.66.1.5', date: '2026-03-28', class: 'network' },
  { name: 'Samsung SSD 980 PRO 1TB', provider: 'Microsoft', version: '10.0.26100.1', date: '2026-01-10', class: 'storage' },
  { name: 'Intel(R) Chipset Device Software', provider: 'Intel Corporation', version: '10.1.19627.8423', date: '2026-04-15', class: 'chipset' },
  { name: 'HID 标准键盘', provider: 'Microsoft', version: '10.0.26100.1', date: '2026-02-20', class: 'input' },
  { name: 'HID-compliant mouse', provider: 'Microsoft', version: '10.0.26100.1', date: '2026-02-20', class: 'input' },
  { name: 'Lenovo T480s Webcam', provider: 'Luxvisions Innovation Limited', version: '10.0.26100.1', date: '2026-03-05', class: 'input' },
];

/** 只读枚举：返回样例驱动副本（避免调用方改动污染模块级数组）。 */
export async function listInstalledDrivers(): Promise<MockInstalledDriver[]> {
  await wait(80);
  return MOCK_DRIVERS.map((d) => ({ ...d }));
}

// —— R186：预览模式的磁盘空间外推演示数据 ——
// 真实桌面端按空间历史外推「预计满盘天数」（check_space_forecast_cmd，只读）；
// 浏览器里没有历史文件可读，直接用 R183 趋势 mock 的口径外推——与趋势卡的
// 「约 112 天满」同源，体检/清理提醒的立即检查不再空手而归。

export async function checkSpaceForecast(): Promise<
  { path: string; days: number | null; reason: string; warn: boolean }[]
> {
  await wait(60);
  // R234：外推天数与磁盘当前值同源——用 listDrives 同一口径（used 随会话
  // 释放台账下压），按历史增速外推满盘天数。清理后磁盘降 → 天数增大，
  // 清理提醒「还能用 112 天」不再是一成不变的死数字。
  const total = 187.1 * GB;
  const used = Math.max(0, 148.1 * GB - sessionReclaimedBytes);
  const free = Math.max(0, total - used);
  // 增速口径：近 14 天平均日增（趋势卡同源：0.42GB/天爬升，第 4 天回落
  // 只影响那一天；外推取净爬升段 ≈0.42GB/天）。
  const dailyGrowth = 0.42 * GB;
  const days = dailyGrowth > 0 ? Math.floor(free / dailyGrowth) : null;
  // R239：预警判定与用户设置联动——读 reminderCfg.full_warn_days（R99 配置，
  // 0=关）。真实后端 space-full 提醒「≤ 阈值天数的盘」按该配置逐盘判，mock
  // 硬编码 <30 会与设置页阈值脱节（设 14 天却按 30 天预警）。
  const threshold = reminderCfg.full_warn_days > 0 ? reminderCfg.full_warn_days : null;
  return [{ path: 'C:\\', days, reason: 'ok', warn: days !== null && threshold !== null && days <= threshold }];
}

// —— R187：预览模式的周体检简报演示数据 ——
// 真实桌面端从扫描建议 + 温度 + 硬盘快照算周报（generate_weekly_report_cmd），
// 浏览器里没有传感器/归档，给一份与 R182/R183 同源的 C 盘样例（可清字节 =
// cleanupSuggestions 演示口径的合计），历史页的周体检卡、环比 delta（R95）、
// 间隔天数（R154）在演示时都有数据可看。

export interface MockWeeklyReport {
  ts: number;
  drives: { path: string; bytes: number }[];
  total_bytes: number;
  temp_max_c: number | null;
  hw_snapshots: number;
  hw_delta: string[];
  disks: { friendly: string; letters: string; health: string; grade: string }[];
}

function weeklyReportAt(ts: number, suggestibleBytes?: number): MockWeeklyReport {
  // R235：周报「C 盘可清字节」与总览可清理卡片同源（cleanupSuggestions
  // 建议项 bytes 合计，days 默认缩放口径）——清理后生成的周报随之下降，
  // 不再是固定 2.91GB 的死数字（真实后端周报从扫描建议汇总，清理后应减少）。
  // 不传时保持 2.91GB 基线：历史样例用它，环比 delta 正好演示清理效果。
  const bytes = suggestibleBytes ?? 2.91 * GB;
  return {
    ts,
    drives: [{ path: 'C:\\', bytes }],
    total_bytes: bytes,
    temp_max_c: 52,
    hw_snapshots: 2,
    hw_delta: ['CPU 最高温 52°C（上周 55°C）', '硬盘通电累计 +168 小时'],
    disks: [{ friendly: 'Samsung SSD 980 PRO 1TB', letters: 'C', health: 'Healthy 100% (Samsung Magician 口径)', grade: 'good' }],
  };
}

const WEEKLY_ARCHIVE: MockWeeklyReport[] = [];

/** 生成一份新周报：写入演示归档（getWeeklyReports 立即能读到）。
 *  R235：可清字节与 cleanupSuggestions 同源——刚生成的周报反映当前可清量
 *  （清理后周报下降），历史样例用基线 2.91GB，环比 delta 演示清理效果。 */
export async function generateWeeklyReport(): Promise<MockWeeklyReport> {
  await wait(150);
  const now = Math.floor(Date.now() / 1000);
  const suggestible = await cleanupSuggestions('C:\\', undefined, true).then((list) =>
    Math.round(list.reduce((s, r) => s + r.bytes, 0)),
  );
  const r = weeklyReportAt(now, suggestible);
  WEEKLY_ARCHIVE.unshift(r);
  if (WEEKLY_ARCHIVE.length > 10) WEEKLY_ARCHIVE.pop();
  return r;
}

/** 最近 N 份周报（最新在前）。未生成过时先出一份上周的样例，让环比一开始可看。
 *  R235：上周样例也用建议合计基线——与本周同源，清理前 delta≈0，清理后
 *  本周下降、delta = 本次清理量（不再出现「比上周多 130 GB」的失真环比）。 */
export async function getWeeklyReports(limit?: number): Promise<MockWeeklyReport[]> {
  await wait(40);
  if (WEEKLY_ARCHIVE.length === 0) {
    const now = Math.floor(Date.now() / 1000);
    const suggestible = await cleanupSuggestions('C:\\', undefined, true).then((list) =>
      Math.round(list.reduce((s, r) => s + r.bytes, 0)),
    );
    WEEKLY_ARCHIVE.push(weeklyReportAt(now - 7 * 86_400, suggestible));
  }
  return WEEKLY_ARCHIVE.slice(0, limit ?? 5).map((r) => ({ ...r }));
}


// —— health-butler P0：建议清理跨次对比的预览演示数据 ——
// 预览模式没有跨会话的历史文件，不能凭空造一个"上次"假装环比。诚实做法：
// 会话内首次询问 = 「首次，无比值」（total_delta=null），与桌面端首次扫描后
// 只有一份快照的表现一致；本轮会话内再次询问同一盘时，与上一份本会话快照
// 比，让用户能在预览里看到真实的环比交互（而不是假数字）。
interface MockSuggestionSnapshot { root: string; total_bytes: number; scaffolds: Record<string, number>; at: number; }

/** 预览演示用的建议环比形状（与 api 层 SuggestionDiff 同构）。 */
export interface MockSuggestionDiff {
  at: number;
  prev_at: number;
  total_delta: number | null;
  gap_days: number | null;
  scaffolds: { scaffold_id: string; now_bytes: number; prev_bytes: number; delta: number }[];
  new_scaffolds: string[];
  gone_scaffolds: string[];
}
const SUGGESTION_SNAPSHOTS: MockSuggestionSnapshot[] = [];

/** 预览模式建议环比（口径与桌面端一致：缺上次 → null，不编造差值）。 */
export async function suggestionDiff(root: string): Promise<MockSuggestionDiff | null> {
  await wait(60);
  const prev = [...SUGGESTION_SNAPSHOTS].reverse().find((s) => s.root === root) ?? null;
  const now = Math.floor(Date.now() / 1000);
  // 与总览建议卡同源：当前可清合计作"本次"快照，再记进会话历史供下次环比。
  const list = await cleanupSuggestions(root, undefined, true);
  const scaffolds: Record<string, number> = {};
  let total_bytes = 0;
  for (const s of list) {
    scaffolds[s.scaffold_id] = (scaffolds[s.scaffold_id] ?? 0) + s.bytes;
    total_bytes += s.bytes;
  }
  SUGGESTION_SNAPSHOTS.push({ root, total_bytes, scaffolds, at: now });
  if (SUGGESTION_SNAPSHOTS.length > 20) SUGGESTION_SNAPSHOTS.shift();
  const shared = prev ? Object.keys(scaffolds).filter((k) => prev.scaffolds[k] !== undefined) : [];
  const newScaffolds = prev ? Object.keys(scaffolds).filter((k) => prev.scaffolds[k] === undefined) : Object.keys(scaffolds);
  const goneScaffolds = prev ? Object.keys(prev.scaffolds).filter((k) => scaffolds[k] === undefined) : [];
  return {
    at: now,
    prev_at: prev?.at ?? 0,
    total_delta: prev ? total_bytes - prev.total_bytes : null,
    gap_days: prev && now > prev.at ? Math.floor((now - prev.at) / 86_400) : null,
    scaffolds: shared
      .map((k) => ({ scaffold_id: k, now_bytes: scaffolds[k], prev_bytes: prev!.scaffolds[k], delta: scaffolds[k] - prev!.scaffolds[k] }))
      .sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta)),
    new_scaffolds: newScaffolds.sort(),
    gone_scaffolds: goneScaffolds.sort(),
  };
}

// —— R196：预览模式的电源计划演示数据 ——
// 真实桌面端 power_plan 无参时枚举 Windows 电源计划（只读）；带
// setScheme 时切换系统计划（系统级写，L2 权限 + 两步确认）。浏览器
// 里没有真实电源状态，读给 3 个标准计划（当前均衡），切换保持
// tauriOnly 拒绝——演示不假装「已切换系统电源计划」。

export async function powerPlan(): Promise<{ active: string | null; schemes: string[] }> {
  await wait(40);
  return {
    active: '381b4222-f694-41f0-9685-ff5bb260df2e',
    schemes: [
      '381b4222-f694-41f0-9685-ff5bb260df2e',
      '8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c',
      'a1841308-3541-4fab-bc81-f71556f20b4a',
    ],
  };
}

// —— R195：预览模式的社区脚本仓库演示数据 ——
// 真实桌面端 scaffold_registry_list 从远端仓库拉社区清理脚本（只读，
// 条目带 Ed25519 签名元数据）；浏览器里没有远端仓库，给 3 条演示脚本
// ——清理模板库「社区脚本」区在演示时能看到签名/风险/下载量，
// 而不是永远「暂无社区脚本」。「一键安装」写操作保持 tauriOnly。

export async function scaffoldRegistryList(): Promise<{
  items: Array<{
    id: string;
    name: string;
    version: string;
    author: string;
    description: string;
    risk: string;
    tags: string[];
    url: string;
    sha256: string;
    signature: string;
    signer: string;
    downloads: number;
  }>;
  warning: string | null;
  updated_at: number;
}> {
  await wait(60);
  return {
    items: [
      {
        id: 'edge-cache-cleaner',
        name: 'Edge 浏览器缓存清理',
        version: '1.3.0',
        author: 'DiskPilot 社区',
        description: '清理 Microsoft Edge 的缓存、临时文件与崩溃转储（浏览数据除外）',
        risk: 'low',
        tags: ['browser', 'cache'],
        url: 'https://registry.example.com/edge-cache-cleaner.zip',
        sha256: '3f8a2b1c9d4e5f60718293a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d',
        signature: 'ed25519:ab12cd34',
        signer: 'DiskPilot 签名密钥',
        downloads: 2314,
      },
      {
        id: 'temp-purge',
        name: '临时文件深度清理',
        version: '2.1.0',
        author: 'xieyang',
        description: '扫描 %TEMP% 与系统 Temp 目录，按最后访问时间清理过期临时文件',
        risk: 'medium',
        tags: ['temp', 'system'],
        url: 'https://registry.example.com/temp-purge.zip',
        sha256: '9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e',
        signature: 'ed25519:ef56ab78',
        signer: 'DiskPilot 签名密钥',
        downloads: 1287,
      },
      {
        id: 'log-cleanup',
        name: '系统日志归档清理',
        version: '0.9.2',
        author: 'DiskPilot 团队',
        description: '归档并清理 Windows 事件日志与旧应用日志（保留近 30 天）',
        risk: 'low',
        tags: ['logs', 'system'],
        url: 'https://registry.example.com/log-cleanup.zip',
        sha256: 'b1c2d3e4f5a6b7c8d9e0f1a2b3c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d',
        signature: 'ed25519:cd78ef90',
        signer: 'DiskPilot 签名密钥',
        downloads: 865,
      },
    ],
    warning: null,
    updated_at: Math.floor(Date.now() / 1000) - 3600,
  };
}

// —— R194：预览模式的 MCP 审计日志演示数据 ——
// 真实桌面端 mcp_audit_tail 返回最近 call_tool / 服务器增删改记录（只读）；
// 浏览器里没有真实调用记录，给与 R188 mcpListServers 同源的几条演示
// 审计——MCP 页「审计」面板在演示时能看到调用/增删记录，而不是
// 永远「暂无审计记录」。

export async function mcpAuditTail(_limit = 50): Promise<
  Array<{
    ts: number;
    op: string;
    server_id: string;
    tool?: string;
    perm?: string;
    granted?: boolean;
    args?: unknown;
    ok?: boolean;
    error?: string | null;
    server_name?: string;
    writable?: boolean;
  }>
> {
  await wait(40);
  const now = Math.floor(Date.now() / 1000);
  return [
    {
      ts: now - 5 * 60,
      op: 'call_tool',
      server_id: 'filesystem',
      server_name: '文件系统',
      tool: 'list_dir',
      perm: 'L0',
      granted: true,
      args: { keys: ['path'] },
      ok: true,
      writable: false,
    },
    {
      ts: now - 25 * 60,
      op: 'call_tool',
      server_id: 'filesystem',
      server_name: '文件系统',
      tool: 'write_file',
      perm: 'L2',
      granted: false,
      args: { keys: ['path', 'content'] },
      ok: false,
      error: '权限不足：write_file 需要 L2 授权',
      writable: true,
    },
    {
      ts: now - 3 * 3600,
      op: 'update_server',
      server_id: 'github',
      server_name: 'GitHub',
      granted: true,
      ok: true,
    },
    {
      ts: now - 2 * 86400,
      op: 'add_server',
      server_id: 'time',
      server_name: '时间服务',
      granted: true,
      ok: true,
    },
  ];
}

// —— R193：预览模式的插件市场演示数据 ——
// 真实桌面端 plugin_market 返回内置 CLI 工具的「已插件化」索引，
// plugin_list_installed 返回已安装台账（含回滚历史）；浏览器里没有
// 真实安装目录，给两条与 R184/R185 mock（启动项/驱动）呼应的演示条目。
// 安装/激活/卸载/更新等写操作保持 tauriOnly 拒绝——真实文件系统写入
// 不演示假成功；社区索引 URL 保持未配置（诚实：写操作无法演示）。

export interface MockMarketPlugin {
  id: string;
  name: string;
  category: string;
  purpose: string;
  risk: string;
  permission_level: string;
  publisher: string;
  exe_rel: string;
  installed: boolean;
}

export async function pluginMarket(): Promise<MockMarketPlugin[]> {
  await wait(50);
  return [
    {
      id: 'disk-cleaner',
      name: '智能清理',
      category: '系统清理',
      purpose: '按内置规则扫描并清理各类缓存与临时文件（回收站可还原）',
      risk: 'medium',
      permission_level: 'L2',
      publisher: 'DiskPilot 团队',
      exe_rel: 'tools/disk-cleaner',
      installed: true,
    },
    {
      id: 'startup-manager',
      name: '启动项管理',
      category: '系统优化',
      purpose: '查看与开关开机自启项（注册表/启动文件夹）',
      risk: 'low',
      permission_level: 'L2',
      publisher: 'DiskPilot 团队',
      exe_rel: 'tools/startup-manager',
      installed: true,
    },
    {
      id: 'driver-checker',
      name: '驱动检测',
      category: '驱动',
      purpose: '列出已装驱动并检查 Windows 更新中的驱动版本',
      risk: 'low',
      permission_level: 'L1',
      publisher: 'DiskPilot 团队',
      exe_rel: 'tools/driver-checker',
      installed: false,
    },
  ];
}

export interface MockLedgerEntry {
  ts: number;
  kind: string;
  id: string;
  name: string;
  version: string;
  source: string;
  dir_rel: string;
  blob: string;
  blob_hash: string;
  signer: string;
  from_version: string;
}

export interface MockInstalledPlugin {
  id: string;
  name: string;
  version: string;
  source: string;
  dir_rel: string;
  installed_at: number;
  signer: string;
  blob: string;
  blob_hash: string;
  history: MockLedgerEntry[];
}

export async function pluginListInstalled(): Promise<{
  installed: MockInstalledPlugin[];
  total_events: number;
}> {
  await wait(60);
  const now = Math.floor(Date.now() / 1000);
  return {
    installed: [
      {
        id: 'disk-cleaner',
        name: '智能清理',
        version: '2.3.1',
        source: 'registry',
        dir_rel: 'tools/disk-cleaner',
        installed_at: now - 45 * 86_400,
        signer: 'DiskPilot',
        blob: '',
        blob_hash: '',
        history: [
          { ts: now - 45 * 86_400, kind: 'install', id: 'disk-cleaner', name: '智能清理', version: '2.3.1', source: 'registry', dir_rel: 'tools/disk-cleaner', blob: '', blob_hash: '', signer: 'DiskPilot', from_version: '' },
          { ts: now - 90 * 86_400, kind: 'update', id: 'disk-cleaner', name: '智能清理', version: '2.2.0', source: 'registry', dir_rel: 'tools/disk-cleaner', blob: '', blob_hash: '', signer: 'DiskPilot', from_version: '2.1.0' },
        ],
      },
      {
        id: 'startup-manager',
        name: '启动项管理',
        version: '1.2.0',
        source: 'url',
        dir_rel: 'tools/startup-manager',
        installed_at: now - 12 * 86_400,
        signer: 'DiskPilot',
        blob: '',
        blob_hash: '',
        history: [
          { ts: now - 12 * 86_400, kind: 'install', id: 'startup-manager', name: '启动项管理', version: '1.2.0', source: 'url', dir_rel: 'tools/startup-manager', blob: '', blob_hash: '', signer: 'DiskPilot', from_version: '' },
        ],
      },
    ],
    total_events: 3,
  };
}

// —— R192：预览模式的清理提醒演示数据 ——
// 真实桌面端提醒配置持久化在 app_data_dir/reminder.json（get/set 同源）；
// 浏览器里没有磁盘文件，用模块级状态模拟这份配置——开关/间隔/阈值在
// 会话内可真实切换（不再点了开关弹回关闭）。
// run_reminder_check_cmd 逐盘查可清量，出提醒负载（只读，绝不执行清理）；
// 浏览器里没有真实空间历史，给 C 盘一条与清理建议 mock 同源的提醒
// （2.91 GB）——「现在检查一次」在演示时能显示检查结果。

export interface MockReminderConfig {
  enabled: boolean;
  interval_hours: number;
  min_bytes: number;
  weekly_report_enabled: boolean;
  weekly_interval_days: number;
  full_warn_days: number;
  /** R297：多久没扫阈值（默认 21，与后端 ReminderConfig 默认同口径） */
  scan_stale_days: number;
}

let reminderCfg: MockReminderConfig = {
  enabled: false,
  interval_hours: 24,
  min_bytes: 200 * 1024 * 1024,
  weekly_report_enabled: false,
  weekly_interval_days: 7,
  full_warn_days: 0,
  scan_stale_days: 21,
};

export async function getReminderConfig(): Promise<MockReminderConfig> {
  await wait(20);
  return { ...reminderCfg };
}

export async function setReminderConfig(patch: Partial<MockReminderConfig>): Promise<MockReminderConfig> {
  await wait(20);
  reminderCfg = { ...reminderCfg, ...patch };
  return { ...reminderCfg };
}

// —— health-butler P2-1：按项授权自动进清单（演示态内存规则集）——
// 真实桌面端持久化到 app_data_dir()/auto-list.json，命中判定在 reminder 循环里
// 顺手累计 scaffold 小计（零额外磁盘扫描）。浏览器里没有真实规则配置文件，
// 用模块级状态模拟，让设置页的规则管理在本会话内可真实增删/切换。
// 语义铁律不变：命中「自动进清单」只影响主动告知，不产生任何执行计划。

export interface MockAutoListRule {
  scaffold_id: string;
  min_bytes: number;
}

export interface MockAutoListRuleView extends MockAutoListRule {
  status: 'ok' | 'unknown_scaffold';
}

export interface MockAutoListConfigView {
  enabled: boolean;
  rules: MockAutoListRuleView[];
}

let autoListCfg: { enabled: boolean; rules: MockAutoListRule[] } = {
  enabled: false,
  rules: [],
};

/** 把规则映射成带状态视图：id 不在已知 scaffolds 中 → unknown_scaffold（不猜近似 id）。 */
function autoListView(known: string[]): MockAutoListConfigView {
  return {
    enabled: autoListCfg.enabled,
    rules: autoListCfg.rules.map((r) => ({
      ...r,
      status: known.includes(r.scaffold_id) ? 'ok' : 'unknown_scaffold',
    })),
  };
}

export async function getAutoListConfigWithStatus(known: string[]): Promise<MockAutoListConfigView> {
  await wait(20);
  return autoListView(known);
}

export async function setAutoListConfig(config: {
  enabled: boolean;
  rules: MockAutoListRule[];
}): Promise<{ enabled: boolean; rules: MockAutoListRule[] }> {
  await wait(20);
  // 覆写语义与后端一致：调用方持有的就是完整规则集；status 由下一次
  // 带已知 scaffold 列表的加载（getAutoListConfigWithStatus）补上。
  autoListCfg = { enabled: config.enabled, rules: config.rules.map((r) => ({ ...r })) };
  return { enabled: autoListCfg.enabled, rules: autoListCfg.rules.map((r) => ({ ...r })) };
}

/** R293 扫描时效：浏览器里没有 scan-stamps.jsonl，也没有真实扫描记录，
 *  返回 null = 「未读到」——不编造「刚扫过」或「从没扫过」两种都不成立的
 *  结论（与 checkSpaceForecast 空列表同口径：没数据就说没数据）。 */
export async function scanStaleness(_root: string): Promise<null> {
  await wait(10);
  return null;
}

export async function runReminderCheck(): Promise<{
  drives: { path: string; bytes: number }[];
  total_bytes: number;
  /** health-butler P2-1 命中项：(scaffold_id, 字节)。演示模式给空（没有真实规则）。 */
  auto_list_hits: [string, number][];
  ts: number;
}> {
  await wait(70);
  // R235：提醒负载「可清字节」与清理建议同源（cleanupSuggestions 合计）——
  // 清理后提醒量随之下降，不再是固定 2.91GB 的死数字（真实后端提醒
  // 从扫描建议汇总，清理后应减少）。rootPath='C:\' 与总览卡片选中盘一致。
  const suggestible = await cleanupSuggestions('C:\\', undefined, true).then((list) =>
    Math.round(list.reduce((s, r) => s + r.bytes, 0)),
  );
  return {
    drives: [{ path: 'C:\\', bytes: suggestible }],
    total_bytes: suggestible,
    auto_list_hits: [],
    ts: Math.floor(Date.now() / 1000),
  };
}

// —— R191：预览模式的硬件信息演示数据 ——
// 真实桌面端从 WMI/PowerShell 读静态硬件信息（hw_info）与磁盘健康
// （hw_disk_health，quick 只跑 WMI）；浏览器里没有真实硬件，给一台与
// 驱动 mock（Intel Iris Xe / Intel Wi-Fi）同源的演示机器——总览体检卡的
// 硬盘健康格不再是「未读到」，工具页硬件聚合面板有完整信息可看。
// 受控压测（hw_run_test）保持 tauriOnly 拒绝——真实负载测试不演示。

export async function hwInfo(): Promise<Record<string, unknown>> {
  await wait(90);
  return {
    cpu: [{ Name: '13th Gen Intel(R) Core(TM) i7-1365U', NumberOfCores: 10, NumberOfLogicalProcessors: 12, MaxClockSpeed: 5200, CurrentClockSpeed: 2893, LoadPercentage: 12 }],
    cpu_usage: [{ PercentProcessorTime: 12 }],
    gpu: [{ Name: 'Intel(R) Iris(R) Xe Graphics', AdapterRAM: 1073741824 }],
    gpu_live: { name: 'Intel(R) Iris(R) Xe Graphics', utilization_pct: 6, temperature_c: 41, memory_used_bytes: 230 * MB, memory_total_bytes: 1024 * MB },
    memory: [{ Capacity: 16 * GB, Speed: 4267, ConfiguredClockSpeed: 4267, Manufacturer: 'Samsung' }],
    system: [{ Manufacturer: 'LENOVO', Model: 'ThinkPad X1 Carbon Gen 10', TotalPhysicalMemory: 16 * GB }],
    bios: [{ Manufacturer: 'LENOVO', SMBIOSBIOSVersion: 'N3XET52W (1.27 )', ReleaseDate: '2025-08-12' }],
    board: [{ Manufacturer: 'LENOVO', Product: '21CB0012CN' }],
    os: [{ Caption: 'Microsoft Windows 11 专业版', OSArchitecture: '64 位', Version: '10.0.26100' }],
    disk: [{ FriendlyName: 'Samsung SSD 980 PRO 1TB', Size: 1024 * GB, MediaType: '3' }],
    thermal: [{ CurrentTemperature: 3251 }],
  };
}

export async function hwDiskHealth(): Promise<Record<string, unknown>> {
  await wait(90);
  return {
    disks: [
      { friendly: 'Samsung SSD 980 PRO 1TB', health: 'Healthy', drive_letters: 'C', reliability: true },
    ],
    reliability: true,
    cdi_report: [{ model: 'Samsung SSD 980 PRO 1TB', health_status: 'Good 100%', temp_c: 42 }],
    note: '演示数据：浏览器预览模式未连接真实 SMART 传感器。',
    substs: [],
  };
}

// —— R190：预览模式的风扇建议演示数据 ——
// 真实桌面端读温度/负载传感器 + 风扇信息算建议档位（fan_curve_advice，
// 只建议不写 EC）；浏览器里没有传感器，给一份与 R187 周报同源（最高温
// 52°C）的演示建议——总览体检卡的温度格不再是「未读到」，系统工具页
// 开「风扇曲线建议」权限后有建议可看。风扇控制（写 EC）保持 tauriOnly
// 拒绝——真实硬件写操作不演示假成功。

export async function fanCurveAdvice(): Promise<{
  level: string;
  level_label: string;
  reason: string;
  temp_max_c: number | null;
  cpu_load_pct: number | null;
  curve: { load_pct: number; fan_pct: number }[];
  fans: Array<Record<string, unknown>>;
  note: string;
}> {
  await wait(60);
  return {
    level: 'balanced',
    level_label: '均衡',
    reason: '当前负载与温度处于均衡区间，风扇按默认曲线运行即可。',
    temp_max_c: 52,
    cpu_load_pct: 34,
    curve: [
      { load_pct: 0, fan_pct: 20 },
      { load_pct: 30, fan_pct: 30 },
      { load_pct: 50, fan_pct: 45 },
      { load_pct: 70, fan_pct: 65 },
      { load_pct: 90, fan_pct: 85 },
      { load_pct: 100, fan_pct: 100 },
    ],
    fans: [{ name: 'CPU 风扇', rpm: 1180, temp_c: 52 }],
    note: '演示数据：浏览器预览模式未连接真实温度传感器。',
  };
}

// —— R189：预览模式的重复文件查找演示数据 ——
// 真实桌面端按 size 分组 + 头部哈希聚类找重复（scan_duplicate_files_cmd，
// 只读）；浏览器里没有真实文件可哈希，给两组确定性样例——组内 ≥2 份、
// keep 建议保留一份、其余进 recycle_candidates——让总览「重复文件查找」
// 的 AI 清理提案窗在演示时能弹出清单（清理执行仍走确认窗铁律）。

export interface MockDupGroup {
  size: number;
  files: { path: string; mtime: number | null; is_running: boolean }[];
  keep: string;
  recycle_candidates: string[];
  running: string[];
}

/**
 * R201：mock「已知文件大小表」——scanDuplicateFiles 的重复组与 estimateSize
 * 共用同一张表，让 CleanupProposalDialog 的逐项实测（estimateSize）在浏览器
 * 演示里命中 mock 文件并显示真实大小，而不是恒 0 → 所有项被误标
 * 「路径不存在 · 约 0 B」。
 */
const MOCK_KNOWN_FILES: Record<string, number> = {
  'C:\\Users\\90740\\Downloads\\installer_x64.exe': 486 * MB,
  'C:\\Users\\90740\\Downloads\\installer_x64 (1).exe': 486 * MB,
  'C:\\Users\\90740\\Downloads\\installer_x64 - 副本.exe': 486 * MB,
  'C:\\Users\\90740\\Videos\\meeting-2026-09.mp4': 121 * MB,
  'C:\\Users\\90740\\Videos\\meeting-2026-09-copy.mp4': 121 * MB,
  'C:\\Users\\90740\\Desktop\\meeting-2026-09.mp4': 121 * MB,
  'C:\\Users\\90740\\Documents\\report-final.docx': 8 * MB,
  'C:\\Users\\90740\\Documents\\report-final-final.docx': 8 * MB,
};

/**
 * R201：浏览器里没有真盘可实测，返回 mock 已知文件的大小；
 * R269：树内真实路径（目录 / file() 叶子）也返回该节点真实大小——
 * 与 executeAiPlan 记账（live 树）同源，确认窗「约 X」= 执行释放 X，
 * 不出现「显示约 0 B、真删释放 1GB」的脱节。未命中表且不在树里返回 0
 * （与后端「路径不存在/空」同语义）。已在回收台账里的路径返回 0
 * （刚才清过 → 再实测就没有了，与后端一致）。
 */
export async function estimateSize(path: string): Promise<number> {
  await wait(40);
  const known = MOCK_KNOWN_FILES[path];
  if (known !== undefined) return known;
  if (!path) return 0;
  // 已回收 → 0（树被剪，节点不存在）。
  if (recycledPaths.some((x) => samePath(x, path))) return 0;
  const tree = pruneRecycledFrom(structuredClone(MOCK_TREE));
  const norm = (s: string) => s.replace(/[\\/]+$/, '').toLowerCase();
  const target = norm(path);
  let found: number | null = null;
  const walk = (n: Node) => {
    if (found !== null) return;
    if (norm(n.path) === target) { found = n.size || 0; return; }
    for (const c of n.children ?? []) walk(c);
  };
  walk(tree);
  return found ?? 0;
}

export async function scanDuplicateFiles(rootPath: string, _minSize?: number): Promise<MockDupGroup[]> {
  await wait(120);
  const now = Math.floor(Date.now() / 1000);
  const groups: MockDupGroup[] = [
    {
      size: 486 * MB,
      files: [
        { path: 'C:\\Users\\90740\\Downloads\\installer_x64.exe', mtime: now - 60 * 86_400, is_running: false },
        { path: 'C:\\Users\\90740\\Downloads\\installer_x64 (1).exe', mtime: now - 30 * 86_400, is_running: false },
        { path: 'C:\\Users\\90740\\Downloads\\installer_x64 - 副本.exe', mtime: now - 12 * 86_400, is_running: false },
      ],
      keep: 'C:\\Users\\90740\\Downloads\\installer_x64.exe',
      recycle_candidates: [
        'C:\\Users\\90740\\Downloads\\installer_x64 (1).exe',
        'C:\\Users\\90740\\Downloads\\installer_x64 - 副本.exe',
      ],
      running: [],
    },
    {
      size: 121 * MB,
      files: [
        { path: 'C:\\Users\\90740\\Videos\\meeting-2026-09.mp4', mtime: now - 45 * 86_400, is_running: false },
        { path: 'C:\\Users\\90740\\Videos\\meeting-2026-09-copy.mp4', mtime: now - 44 * 86_400, is_running: false },
        { path: 'C:\\Users\\90740\\Desktop\\meeting-2026-09.mp4', mtime: now - 43 * 86_400, is_running: false },
      ],
      keep: 'C:\\Users\\90740\\Videos\\meeting-2026-09.mp4',
      recycle_candidates: [
        'C:\\Users\\90740\\Videos\\meeting-2026-09-copy.mp4',
        'C:\\Users\\90740\\Desktop\\meeting-2026-09.mp4',
      ],
      running: [],
    },
    {
      size: 8 * MB,
      files: [
        { path: 'C:\\Users\\90740\\Documents\\report-final.docx', mtime: now - 20 * 86_400, is_running: false },
        { path: 'C:\\Users\\90740\\Documents\\report-final-final.docx', mtime: now - 19 * 86_400, is_running: false },
      ],
      keep: 'C:\\Users\\90740\\Documents\\report-final.docx',
      recycle_candidates: ['C:\\Users\\90740\\Documents\\report-final-final.docx'],
      running: [],
    },
  ];
  // R237：与真实后端同一语义——重复查找只扫选中盘（rootPath 前缀内的文件
  // 才可能被看到），且与回收台账同源：已回收路径（含其目录下的文件）不再
  // 出现在重复组里。剪完不足 2 份的组整组丢掉（只剩 1 份就不是重复）。
  const root = rootPath.replace(/[\\/]+$/, '').toLowerCase();
  const recycled = recycledPaths.map((p) => p.replace(/[\\/]+$/, '').toLowerCase());
  const underRoot = (p: string) => p.toLowerCase().startsWith(root);
  const recycledHere = (p: string) => {
    const low = p.replace(/[\\/]+$/, '').toLowerCase();
    return recycled.some((r) => low === r || low.startsWith(r + '\\') || low.startsWith(r + '/'));
  };
  const out: MockDupGroup[] = [];
  for (const g of groups) {
    const kept = g.files.filter((f) => underRoot(f.path) && !recycledHere(f.path));
    if (kept.length < 2) continue;
    const keptSet = new Set(kept.map((f) => f.path));
    const keep = keptSet.has(g.keep) ? g.keep : kept[0].path;
    const candidates = kept.filter((f) => f.path !== keep).map((f) => f.path);
    if (candidates.length === 0) continue;
    out.push({ ...g, files: kept, keep, recycle_candidates: candidates });
  }
  return out;
}

// —— R188：预览模式的 MCP 服务器演示数据 ——
// 真实桌面端从 mcp.rs 读服务器配置 + 健康探测（mcp_list_servers）；浏览器里
// 没有后端进程可连，给 2 个样例服务器（stdio/http 各一 + 工具数 + 权限摘要），
// 设置页 MCP tab 不再空表。只读 mock；增删改/调用走 tauriOnly 拒绝（真后端
// 副作用不演示假成功）。

export interface MockMcpServerStatus {
  id: string;
  name: string;
  enabled: boolean;
  writable: boolean;
  tool_prefix?: string | null;
  transport_type: 'stdio' | 'http';
  transport: { type: 'stdio' | 'http'; command?: string; args?: string[]; env?: Record<string, string>; cwd?: string | null; url?: string; headers?: Record<string, string>; timeout_secs?: number | null };
  tool_count: number | null;
  error: string | null;
  perm_scope: string;
  permission_map?: Record<string, string> | null;
}

export async function mcpListServers(): Promise<MockMcpServerStatus[]> {
  await wait(60);
  return [
    {
      id: 'mock-mcp-fs', name: 'filesystem', enabled: true, writable: true, tool_prefix: null,
      transport_type: 'stdio',
      transport: { type: 'stdio', command: 'npx', args: ['-y', '@modelcontextprotocol/server-filesystem', 'D:/data'], cwd: null, timeout_secs: 30 },
      tool_count: 7, error: null,
      perm_scope: '3 个 L0 / 4 个 L1',
      permission_map: { list_allowed_directories: 'L0', read_text_file: 'L0', write_text_file: 'L1', create_directory: 'L1' },
    },
    {
      id: 'mock-mcp-time', name: 'time', enabled: true, writable: false, tool_prefix: null,
      transport_type: 'http',
      transport: { type: 'http', url: 'https://mcp.time.com/sse', headers: {}, timeout_secs: 15 },
      tool_count: 4, error: null,
      perm_scope: '4 个 L0',
      permission_map: { get_current_time: 'L0', get_timezone: 'L0' },
    },
    {
      id: 'mock-mcp-disabled', name: 'github', enabled: false, writable: false, tool_prefix: null,
      transport_type: 'http',
      transport: { type: 'http', url: 'https://api.github.com/mcp', headers: {}, timeout_secs: 15 },
      tool_count: null, error: '已停用（未探测）',
      perm_scope: '—',
      permission_map: null,
    },
  ];
}

export async function inspect(path: string, n: number): Promise<string[]> {
  await wait(150);
  const lower = path.toLowerCase();
  if (lower.includes('edge')) {
    return [
      `${path}\\Default\\Cache\\Cache_Data\\f_001234`,
      `${path}\\Default\\Cache\\Cache_Data\\f_005a2b`,
      `${path}\\Default\\Code Cache\\js\\index-DHc9.bin`,
      `${path}\\Default\\GPUCache\\data_3`,
      `${path}\\Default\\Service Worker\\CacheStorage\\...`,
    ].slice(0, n);
  }
  if (lower.includes('huggingface')) {
    return [
      `${path}\\hub\\models--meta-llama--Llama-3.1-8B-Instruct\\blobs\\...`,
      `${path}\\hub\\models--openai--clip-vit-base-patch32\\snapshots\\...`,
    ].slice(0, n);
  }
  return Array.from({ length: Math.min(n, 12) }, (_, i) => `${path}\\sample_${i + 1}.bin`);
}

export async function recyclePaths(paths: string[], reason: string): Promise<UndoEntry[]> {
  await wait(400);
  // R270：与 executeScope / executeAiPlan 记账同源——bytes_freed 取树内 /
  // 已知文件表真实大小（prune 后取，已回收的路径自然取不到 → null），
  // 真删累加 sessionReclaimedBytes + reclaimedBySource，磁盘卡 used 与
  // 空间趋势「今天」随右键回收联动下降（此前只记台账不记账，右键清完
  // 卡不降，与清理页真删后卡降的观感不一致）。
  const tree = pruneRecycledFrom(structuredClone(MOCK_TREE));
  const sizeOf = (p: string): number | null => {
    const known = MOCK_KNOWN_FILES[p];
    if (known !== undefined) return known;
    const norm = (s: string) => s.replace(/[\\/]+$/, '').toLowerCase();
    const target = norm(p);
    let found: number | null = null;
    const walk = (n: Node) => {
      if (found !== null) return;
      if (norm(n.path) === target) { found = n.size || 0; return; }
      for (const c of n.children ?? []) walk(c);
    };
    walk(tree);
    return found;
  };
  // R272：只对「磁盘上真实存在」的路径回收——树内节点或已知文件表命中；
  // 树外路径（不存在/已回收被剪）不产生条目，不假装「删成功了」。与
  // executeAiPlan（R267）同一诚实语义：调用方按返回条目数计成功、按
  // bytes_freed 记实际释放，未知路径如实计失败。
  const entries = paths.map((src) => {
    const bytes = sizeOf(src);
    if (bytes === null) return null;
    return {
      timestamp: new Date().toISOString(),
      action: 'recycle' as const,
      source: src,
      destination: null,
      reason,
      bytes_freed: bytes > 0 ? bytes : null,
    };
  }).filter((e): e is NonNullable<typeof e> => e !== null);
  UNDO_LEDGER.push(...entries);
  for (const e of entries) {
    const freed = e.bytes_freed ?? 0;
    sessionReclaimedBytes += freed;
    const key = e.source.replace(/[\\/]+$/, '').toLowerCase();
    reclaimedBySource.set(key, (reclaimedBySource.get(key) ?? 0) + freed);
  }
  // R268：与 executeScope（R225）/ executeAiPlan（R267）同一收敛语义——
  // 回收成功的路径也进 mock 级回收台账，后续 scan/cleanupSuggestions/
  // scopeSizesBatch 基于修剪后的树返回，总览「可清理」不会把刚右键回收
  // 的目录再建议一遍（前端 applyRecycledPrune 剪的是内存树，mock 级
  // 台账管的是演示建议数据源，两者都要剪才一致）。
  for (const e of entries) _mockRecyclePath(e.source);
  return entries;
}

/**
 * mock 台账：与后端 undo.jsonl 同语义（最新在前），回收动作会真的追加进来，
 * 所以「回收 → 历史页看到 → 给出还原出口」这条链路在浏览器 mock 模式里可跑通。
 */
const UNDO_LEDGER: UndoEntry[] = [
  {
    timestamp: new Date(Date.now() - 2 * 86_400_000).toISOString(),
    action: 'quarantine',
    source: 'C:\\Users\\demo\\Documents\\旧报告.docx',
    destination: 'C:\\Users\\demo\\AppData\\Roaming\\diskpilot\\quarantine\\旧报告.docx',
    reason: '隔离而非删除',
    bytes_freed: 3.2 * GB,
  },
  {
    timestamp: new Date(Date.now() - 5 * 3_600_000).toISOString(),
    action: 'recycle',
    source: 'D:\\SteamLibrary\\steamapps\\common\\Leftover Game',
    reason: 'Steam 卸载残留',
    bytes_freed: 12 * GB,
  },
  {
    timestamp: new Date(Date.now() - 86_400_000).toISOString(),
    action: 'delete',
    source: 'C:\\Users\\demo\\AppData\\Local\\Temp\\crash.log',
    reason: '临时日志',
    bytes_freed: 120 * 1024 * 1024,
  },
];

export async function listUndo(limit?: number): Promise<UndoEntry[]> {
  await wait(120);
  const sorted = [...UNDO_LEDGER].sort((a, b) => b.timestamp.localeCompare(a.timestamp));
  return limit && limit > 0 ? sorted.slice(0, limit) : sorted;
}

/** 只有隔离条目能原地还原（与后端 restore_quarantined 同口径）；回收条目返回 null。
 *  R230：还原成功后把 source 从 mock 级回收台账移除——被还原的目录回到
 *  scan / cleanupSuggestions / scopeSizesBatch 的树里（「还原了，树里回来」），
 *  演示里还原与清理是互逆操作，不会出现「还原了但工作台还缺这个目录」。 */
export async function undo(index: number, source: string): Promise<UndoEntry | null> {
  await wait(300);
  const sorted = [...UNDO_LEDGER].sort((a, b) => b.timestamp.localeCompare(a.timestamp));
  const e = sorted[index];
  if (!e || e.action !== 'quarantine' || e.source !== source) return null;
  const at = UNDO_LEDGER.indexOf(e);
  if (at >= 0) UNDO_LEDGER.splice(at, 1);
  // R230：还原 = 文件回到原位，目录从回收台账回来（仅当该目录确实被清过）。
  const idx = recycledPaths.findIndex((p) => samePath(p, source));
  if (idx >= 0) recycledPaths.splice(idx, 1);
  // R233：还原 = 空间回涨——只撤回该 source 实际入账过的释放字节（种子
  // 隔离条目从没入账，还原不回涨；真实后端还原隔离文件后磁盘 used 增加，
  // 演示不能「还原了但空间没回来」，也不能把别的清理释放凭空回涨）。
  const key = e.source.replace(/[\\/]+$/, '').toLowerCase();
  const accounted = reclaimedBySource.get(key) ?? 0;
  if (accounted > 0) {
    reclaimedBySource.delete(key);
    sessionReclaimedBytes = Math.max(0, sessionReclaimedBytes - accounted);
  }
  return e;
}

function wait(ms: number) { return new Promise<void>((r) => setTimeout(r, ms)); }

// —— 预览模式（非 Tauri）清理演示数据 ——
// 真实桌面端走后端 scope_sizes_batch / execute_scope；浏览器里没有磁盘可扫，
// 这些 mock 让「勾选 scope → 预览清单 → 真删 → 进度条/取消 → 历史台账」整条
// 链路在浏览器里可点可看（数字是演示数据，与 MOCK_TREE 的目录规模同量级）。
// 用 scope_id 做确定性哈希：同一 scope 每次返回同一批尺寸，界面刷新不跳变。

function hashString(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/** scope 的演示体量：50MB–2.5GB，按 id 哈希稳定。 */
function mockScopeBytes(scopeId: string, days?: number): number {
  const h = hashString(scopeId);
  const base = (30 + ((h % 220) / 100)) * 10 * MB * 0.5 + (h % 170) * MB;
  // R212：接保留天数联动——真实后端 scope_sizes_batch 带 scopeDays 时按
  // mtime 过滤，天数越大可清越少。mock 线性缩放：0 天=全清，≥30 天封底
  // 30%，让预览里调天数能看到「待清」字节真的变化（此前 mock 忽略
  // scopeDays，改天数字节纹丝不动，演示上天数选择形同虚设）。
  if (days === undefined || days === null || !Number.isFinite(days) || days <= 0) return base;
  const ratio = Math.max(0.3, 1 - Math.min(days, 30) / 30 * 0.7);
  return Math.round(base * ratio);
}

function mockScopeFileCount(scopeId: string): number {
  return 40 + (hashString(scopeId + ':n') % 4000);
}

/** R227：MOCK_TREE 里某 scaffold 的「真实命中总字节」（修剪后）。口径同源：
*  清理页所有 scope 行总和 ≈ 工作台树里该目录的 size，不会再出现
*  「工作台 0.9 GB / 清理页 75 MB / 总览 180 MB」三处数字对不上。
*  找不到该 scaffold 的节点（未检测到）时返回哈希兜底，保证列表恒有值。
*  R228：按 scope 的 glob 目录名拆分子目录——crash-dumps 的三个 scope 行
*  各显示自己的真实目录（CrashDumps / WER / Minidump），不再共享总量。 */
function scaffoldDirBytes(scaffoldId: string, rootPath?: string): Map<string, number> {
  const tree = pruneRecycledFrom(structuredClone(MOCK_TREE));
  const root = rootPath ? rootPath.replace(/[\\/]+$/, '').toLowerCase() : null;
  const byName = new Map<string, number>();
  const walk = (n: Node) => {
    if (n.scaffold_id === scaffoldId) {
      // R228：带 rootPath 时只累计位于该命中目录之下的节点——多个命中
      // 目录（CrashDumps / WER / Minidump）各算各的，aggregateScopeSizes
      // 累加后不再翻倍。
      if (root === null) {
        const name = n.name;
        byName.set(name, (byName.get(name) ?? 0) + (n.size || 0));
      } else {
        const p = n.path.replace(/[\\/]+$/, '').toLowerCase();
        if (p === root || p.startsWith(root + '\\')) {
          const name = n.name;
          byName.set(name, (byName.get(name) ?? 0) + (n.size || 0));
        }
      }
    }
    for (const c of n.children ?? []) walk(c);
  };
  walk(tree);
  return byName;
}

/** scope 的真实可清字节：优先按 glob 里的目录名（CrashDumps/WER/Minidump）
 *  取该目录的树字节；目录名匹配不到时退回 scaffold 总量；再退回哈希。
 *  R228：rootPath 限定命中目录范围——同一 scope 在多个命中目录下时各算
 *  各的；带 rootPath 且该 scope 的目录不在此 root 下时返回 0（该目录
 *  没有这个 scope 的命中，aggregate 累加才不翻倍）。 */
function scopeRealBytes(scaffoldId: string, scopeId: string, glob: string, rootPath?: string): number | null {
  const byName = scaffoldDirBytes(scaffoldId, rootPath);
  if (byName.size === 0) return null;
  // glob 里最具体的目录名（如 CrashDumps / WER / Minidump）→ 匹配树节点名。
  const parts = glob.split(/[\\/]/).filter(Boolean);
  for (let i = parts.length - 1; i >= 0; i--) {
    const name = parts[i];
    // R228：跳过通配段与 brace 段（`**`、`{A,B}` 都不是目录名）。
    if (name.includes('*') || name.includes('{') || name.includes('}')) continue;
    if (byName.has(name)) return byName.get(name)!;
  }
  // R228：rootPath 过滤后目录表非空但 glob 目录名不在其中——该命中目录
  // 与这个 scope 无关，返回 0（不退回 scaffold 总量，否则多个目录聚合翻倍）。
  if (rootPath !== undefined) return 0;
  let total = 0;
  for (const v of byName.values()) total += v;
  return total;
}

/** dry-run / 真删的演示命中条目：路径拼在扫描命中目录下，文件名按 scope 稳定。 */
/** R225：mock 模块级「已回收路径」台账——真删后 scan / cleanupSuggestions /
 *  scopeSizesBatch 都基于修剪后的树返回，总览建议项与工作台体量在演示里
 *  与 store 前端剪枝同源收敛（真实后端剪枝发生在内存树，这里等价模拟）。 */
let recycledPaths: string[] = [];

export function _mockRecyclePath(p: string): void {
  if (!p) return;
  if (!recycledPaths.some((x) => samePath(x, p))) recycledPaths.push(p);
}

/** 测试专用：清空 mock 级回收台账（各 describe 互不污染）。 */
export function _mockResetRecycled(): void {
  recycledPaths = [];
}

/** R230：测试专用——注入一条台账条目（quarantine 类），验证还原回补树。
 *  真实链路里 quarantine 条目由后端 restore_quarantined 产生；mock 的
 *  scaffolds 全是 recycle mode，预览模式无法自然生成隔离条目，测试注入
 *  一条以覆盖「还原 = 目录回补扫描树」的演示语义。 */
export function _mockPushUndoEntry(e: import('./types').UndoEntry): void {
  UNDO_LEDGER.push(e);
}

/** R267：AI 提案执行（executeAiPlan）的 mock——浏览器演示里「AI 提案立即
 *  清理」不再恒空数组必全败。语义对齐真实后端 execute_ai_plan：
 *  - dryRun=true：只返回「可回收」条目（树内真实存在且未被剪过的路径），
 *    不做任何破坏性动作——AI 确认窗先 dry-run 探测、整批可执行才继续。
 *  - dryRun=false：每个可回收路径生成 recycle 台账条目 + 记入 mock 级
 *    回收台账（后续 scan/cleanupSuggestions/scopeSizesBatch 基于修剪后的树
 *    返回，与 executeScope/recyclePaths 同一收敛语义），bytes_freed 用树内
 *    真实大小，未命中树/已剪过的路径返回空（与后端「路径不存在 → 无条目」
 *    诚实一致，不编造释放字节）。
 *  - 危险路径（盘根 / Windows / Program Files）与 AI 清单里被后端拦截的
 *    路径一样不产生条目——mock 不替后端做拦截之外的放行。 */
export async function executeAiPlan(
  selectedPaths: string[],
  userConfirmed: boolean,
  dryRun: boolean,
): Promise<UndoEntry[]> {
  await wait(300);
  // 与后端同步（R289）：dryRun 是纯只读预演，不要求确认——AI 要能自己核实
  // "这条路径真的命中吗、能清多少"，才不会把编造的路径当成事实交给用户。
  // 真执行仍旧必须 confirmed（唯一放行条件）。
  if (!dryRun && !userConfirmed) return Promise.reject(new Error('AI 清理提案需要用户确认后才可执行'));
  if (selectedPaths.length === 0) return [];
  // 快照一份「当前可回收」集合：树内真实存在且未被剪过。树里只有目录 +
  // 少量 file() 叶子，重复文件候选在 MOCK_KNOWN_FILES 表（只读扫描/
  // estimateSize 同源）——两处都算「磁盘上真实存在的路径」，任一命中即可。
  const tree = pruneRecycledFrom(structuredClone(MOCK_TREE));
  const live = new Map<string, number>();
  const walk = (n: Node) => {
    live.set(n.path.replace(/[\\/]+$/, '').toLowerCase(), n.size || 0);
    for (const c of n.children ?? []) walk(c);
  };
  walk(tree);
  for (const [p, size] of Object.entries(MOCK_KNOWN_FILES)) {
    live.set(p.replace(/[\\/]+$/, '').toLowerCase(), size);
  }
  // R267：已回收路径从 live 排除（重复文件执行一次后再执行，第二次 dry-run
  // 就空——与「真清了，再次扫描没得清」一致，不会同一文件重复入账）。
  for (const rp of recycledPaths) {
    live.delete(rp.replace(/[\\/]+$/, '').toLowerCase());
  }
  const dangerous = /^(?:[a-z]:$|\\\\|.*[\\/](?:Windows|Program Files(?: \(x86\))?)(?:[\\/]|$))/i;
  // 盘根单独判定：C:\（归一后 c:）是危险路径，C:\Users\... 不是。
  const isRoot = /^[a-z]:[\\/]$/i;
  const hits: UndoEntry[] = [];
  for (const raw of selectedPaths) {
    const p = String(raw ?? '').trim();
    if (!p) continue;
    if (isRoot.test(p) || dangerous.test(p)) continue; // 后端拦截高危路径，mock 同口径不放行
    const key = p.replace(/[\\/]+$/, '').toLowerCase();
    const bytes = live.get(key);
    if (bytes === undefined) continue; // 树里没有 → 该路径不存在/已清，诚实跳过
    const entry: UndoEntry = {
      timestamp: new Date().toISOString(),
      action: 'recycle',
      source: p,
      destination: null,
      reason: 'AI 清理提案确认后回收',
      bytes_freed: bytes > 0 ? bytes : null,
    };
    hits.push(entry);
    if (!dryRun) {
      UNDO_LEDGER.push(entry);
      _mockRecyclePath(p);
      // R271：与 executeScope（R231-R233）/ recyclePaths（R270）记账同构——
      // AI 提案确认执行后磁盘卡 used 与空间趋势「今天」也下降，不带
      // 「清完了空间没回来」的观感脱节（此前只记台账不记账）。
      const freed = entry.bytes_freed ?? 0;
      sessionReclaimedBytes += freed;
      reclaimedBySource.set(key, (reclaimedBySource.get(key) ?? 0) + freed);
    }
  }
  return hits;
}

function samePath(a: string, b: string): boolean {
  return a.replace(/[\\/]+$/, '').toLowerCase() === b.replace(/[\\/]+$/, '').toLowerCase();
}

/** 把已回收路径从一棵克隆树上剪掉（整目录移除；祖先 size 聚合自动扣减）。 */
function pruneRecycledFrom(tree: Node): Node {
  if (recycledPaths.length === 0) return tree;
  const targets = new Set(recycledPaths.map((p) => p.replace(/[\\/]+$/, '').toLowerCase()));
  const walk = (n: Node): Node => {
    let removedSize = 0;
    let removedFiles = 0;
    const kids: Node[] = [];
    for (const c of n.children ?? []) {
      if (targets.has(c.path.replace(/[\\/]+$/, '').toLowerCase())) {
        removedSize += c.size || 0;
        removedFiles += c.file_count || 0;
        continue;
      }
      const nc = walk(c);
      removedSize += Math.max(0, (c.size || 0) - (nc.size || 0));
      removedFiles += Math.max(0, (c.file_count || 0) - (nc.file_count || 0));
      kids.push(nc);
    }
    if (removedSize === 0 && removedFiles === 0) return n;
    return {
      ...n,
      children: kids,
      size: Math.max(0, (n.size || 0) - removedSize),
      file_count: Math.max(0, (n.file_count || 0) - removedFiles),
    };
  };
  return walk(tree);
}

/** R225：从 MOCK_TREE 收集指定 scaffold 的真实命中节点路径（与扫描树同源）。
 *  真删条目用这些路径做 source——R222/R224 的前端剪枝按 source 从 root/
 *  scanCache 剪掉，演示里执行后总览/工作台体量才能真实下降。
 *  只收集「位于 rootPath 之下」的命中，避免跨盘误剪（treeMayContain 前缀判定）。 */
/** 收集某 scope 在 rootPath 下的真实命中目录路径（执行时只剪这个 scope
 *  的目录，不误伤同 scaffold 的其它 scope 目录——R229：清 WER 不该把
 *  CrashDumps/Minidump 一起剪掉）。 */
function mockRealScopePaths(scaffoldId: string, scopeId: string, rootPath: string): string[] {
  const root = rootPath.replace(/[\\/]+$/, '').toLowerCase();
  const scope = SCAFFOLDS.find((s) => s.id === scaffoldId)?.scopes.find((x) => x.id === scopeId);
  if (!scope) return [];
  // scope.glob 里的目录名（CrashDumps/WER/Minidump）——该 scope 只剪这些目录。
  const scopeDirs = scope.glob
    .split(/[\\/]/)
    .filter((seg) => seg && !seg.includes('*') && !seg.includes('{') && !seg.includes('}'))
    .map((seg) => seg.toLowerCase());
  const out: string[] = [];
  const walk = (n: Node) => {
    if (n.path && n.scaffold_id === scaffoldId) {
      const p = n.path.replace(/[\\/]+$/, '').toLowerCase();
      if (p === root || p.startsWith(root + '\\')) {
        // 目录名必须是该 scope 的 glob 目录之一（大小写不敏感）
        if (scopeDirs.includes(n.name.toLowerCase())) out.push(n.path);
      }
    }
    for (const c of n.children ?? []) walk(c);
  };
  walk(MOCK_TREE);
  return out;
}

function mockScopeEntries(scopeId: string, rootPath: string, n: number, days?: number, realPaths?: string[], realBytes?: number | null): UndoEntry[] {
  // R227：真删/预览的释放字节与扫描树同源（realBytes = 该 scope 目录真实
  // 可清量）；无真实基准确认时空值退回哈希，保证演示口径不回退。
  const bytes =
    realBytes !== undefined && realBytes !== null
      ? (days === undefined ? realBytes : Math.round(realBytes * Math.max(0.3, 1 - Math.min(days, 30) / 30 * 0.7)))
      : mockScopeBytes(scopeId, days);
  const per = Math.max(1, Math.floor(bytes / Math.max(1, n)));
  const stem = hashString(scopeId + ':stem');
  const out: UndoEntry[] = [];
  for (let i = 0; i < n; i++) {
    // R225：真删优先用真实树节点路径本身（回收目录语义）——pruneMany 按
    // 节点路径匹配整目录移除，与「回收了该目录」一致；同一目录可被多个
    // 条目引用（演示按文件数展开清单），剪枝去重后只剪一次。
    // 真实命中不足时用合成路径兜底（演示口径不变）。
    const source =
      realPaths && realPaths.length > 0
        ? realPaths[i % realPaths.length]
        : `${rootPath}\\${scopeId}\\mock_${stem % 13}_chunk_${i + 1}`;
    out.push({
      timestamp: new Date().toISOString(),
      action: 'recycle' as const,
      source,
      reason: scopeId,
      bytes_freed: i === 0 ? bytes - per * (n - 1) : per,
    });
  }
  return out;
}

export async function scopeSizesBatch(
  scaffoldId: string,
  rootPaths: string[],
  scopeDays?: Record<string, number>,
): Promise<{ root_path: string; sizes: { scope_id: string; bytes: number; file_count: number; total_bytes: number; total_files: number }[] }[]> {
  await wait(50);
  void scopeDays;
  const scaffold = SCAFFOLDS.find((s) => s.id === scaffoldId);
  // R227：优先用 MOCK_TREE 真实命中字节（修剪后）——清理页 scope 行不再
  // 用哈希公式，数字与工作台树的目录 size 同源。找不到真实节点时退回
  // 哈希兜底（未检测到的脚本也能展示列表）。
  // R228：每个 scope 行按自己的 glob 目录名取真实字节（crash-dumps 的
  // CrashDumps / WER / Minidump 三个目录各自显示，不再共享总量）。
  // 每个 root_path 是独立的命中目录——sizes 在 map 内按各自 rootPath
  // 过滤，aggregate 累加多个目录时才不会把同一目录算两遍。
  return rootPaths.map((p) => {
    const sizes = (scaffold?.scopes ?? []).map((sc) => {
      // R212：接入保留天数——只有带 prompt.days 的 scope 按用户天数缩放可清字节
      // （真实后端 scope_sizes_batch 带 scopeDays 时按 mtime 过滤；无保留期的
      // scope 不受 scopeDays 影响）。天数越大可清越少，封底 30%。
      const days = sc.prompt?.kind === 'days' ? (scopeDays?.[sc.id] ?? sc.prompt.default ?? 30) : undefined;
      // R227：真实字节同源——total_bytes 显示目录全量（真实后端是该 scope
      // 全部命中），bytes 显示按保留天数过滤后的可清量（R212 天数缩放）。
      const real = scaffold !== undefined ? scopeRealBytes(scaffoldId, sc.id, sc.glob, p) : null;
      const raw = real ?? mockScopeBytes(sc.id);
      const bytes = days === undefined ? raw : Math.round(raw * Math.max(0.3, 1 - Math.min(days, 30) / 30 * 0.7));
      const files = mockScopeFileCount(sc.id);
      return {
        scope_id: sc.id,
        bytes,
        file_count: files,
        total_bytes: raw,
        total_files: files,
      };
    });
    return { root_path: p, sizes };
  });
}

/** 演示执行：dry-run 返回样例清单（条目数 = 命中体量换算）；真删追加到台账并返回回收条目。 */
export async function executeScope(
  scaffoldId: string,
  scopeId: string,
  rootPath: string,
  dryRun: boolean,
  opts: { olderThanDays?: number | null; wxidFilter?: string[] | null; envFilter?: string[] | null; confirmed?: boolean | null } = {},
): Promise<UndoEntry[]> {
  void scaffoldId;
  await wait(dryRun ? 60 : 120);
  // R232：olderThanDays 未显式传时取 scope 的 prompt.default——执行释放
  // 字节与清理页「待清」（scopeSizesBatch 同源缩放）一致：真实后端执行
  // 时用户保留天数就是 scope 的默认/面板设定，不是「不清除地全删」。
  const days = (() => {
    if (opts.olderThanDays !== undefined && opts.olderThanDays !== null) return opts.olderThanDays;
    const sc = SCAFFOLDS.find((s) => s.id === scaffoldId)?.scopes.find((x) => x.id === scopeId);
    return sc?.prompt?.kind === 'days' ? (sc.prompt.default ?? 30) : undefined;
  })();
  // R243：分片量 n 与「真正会释放的字节」同源（days 缩放后的真实字节）——
  // 此前 n 用哈希 mockScopeBytes 兜底，12GB 目录也可能只分 4 条进度，
  // 进度条「36/64 条」与预览合计脱节。现在按实际释放量换算：每 ~24MB 一条。
  const effBytes = (() => {
    const sc = SCAFFOLDS.find((s) => s.id === scaffoldId)?.scopes.find((x) => x.id === scopeId);
    const real = sc ? scopeRealBytes(scaffoldId, scopeId, sc.glob) : null;
    const raw = real ?? mockScopeBytes(scopeId);
    return days === undefined ? raw : Math.round(raw * Math.max(0.3, 1 - Math.min(days, 30) / 30 * 0.7));
  })();
  const n = Math.min(64, Math.max(4, Math.ceil(effBytes / (24 * MB))));
  // R225：真删时收集真实树路径（dry-run 保持合成清单，预览不受影响）。
  // R229：只剪这个 scope 的目录（CrashDumps/WER/Minidump 各是各的），
  // 清 WER 不再把同 scaffold 的 CrashDumps/Minidump 一起剪掉。
  const realPaths = dryRun ? undefined : mockRealScopePaths(scaffoldId, scopeId, rootPath);
  // R273：真删但 rootPath 下没有该 scope 命中目录（清过的 scope 再执行 /
  // root 不含该目录）→ 返回空数组，与真实后端 execute_scope 无匹配文件
  // 时返回空同一语义（调用方走 nothingMatched，不假装「删了虚拟路径」）。
  // dry-run 不受影响：预览清单仍是演示样例（用户先看预览，执行时才诚实）。
  if (!dryRun && (!realPaths || realPaths.length === 0)) {
    return [];
  }
  // R227：真删/预览的释放字节与扫描树同源（该 scope 目录真实可清量）。
  // R228：按 scope 的 glob 目录名取自己的真实字节（CrashDumps/WER/Minidump）。
  const realBytes = (() => {
    const sc = SCAFFOLDS.find((s) => s.id === scaffoldId)?.scopes.find((x) => x.id === scopeId);
    // R228：executeScope 的 rootPath 是扫描根（可能覆盖多个目录），释放
    // 字节取该 scope 目录全局真实大小（与 scopeSizesBatch/cleanupSuggestions
    // 同源），不做 rootPath 过滤。
    return sc ? scopeRealBytes(scaffoldId, scopeId, sc.glob) : null;
  })();
  const entries = mockScopeEntries(scopeId, rootPath, n, days, realPaths, realBytes);
  if (!dryRun) {
    UNDO_LEDGER.push(...entries);
    // R231：真删释放字节累加进会话台账——空间趋势卡今天回落与本次清理同源。
    // R232：按执行条目 bytes_freed 和累加（= days 过滤后实际删的量），与
    // 总览 toast「已释放 X」一致——清 276MB 就降 276MB，不是降整目录 0.9GB。
    // R233：同时记 per-source 入账——还原时只撤回该 source 的部分。
    for (const e of entries) {
      const freed = e.bytes_freed ?? 0;
      sessionReclaimedBytes += freed;
      const key = e.source.replace(/[\\/]+$/, '').toLowerCase();
      reclaimedBySource.set(key, (reclaimedBySource.get(key) ?? 0) + freed);
    }
    // R225：真删路径进 mock 级台账——后续 scan / cleanupSuggestions 从
    // 修剪后的树返回，演示里「清完再看」各处体量一致下降。
    if (realPaths) for (const p of realPaths) _mockRecyclePath(p);
  }
  return entries;
}

/** R182：总览页「可清理」卡片的演示建议。按 rootPath 前缀在 MOCK_TREE 里找
 *  命中的 scaffold 目录（与扫描命中的 scaffold 同源），每个脚本挑最多 2 个
 *  cache/media scope 生成建议项——字节沿用 mockScopeBytes，与清理页 scope 行、
 *  执行 mock 同一口径（总览显示的大小 = 执行时真正清的大小）。 */
function collectScaffoldHits(node: Node, rootPath: string, out: Map<string, string>): void {
  const root = rootPath.toLowerCase();
  if (node.path && node.path.toLowerCase().startsWith(root)) {
    if (node.scaffold_id && !out.has(node.scaffold_id)) out.set(node.scaffold_id, node.path);
  }
  for (const c of node.children ?? []) collectScaffoldHits(c, rootPath, out);
}

export interface CleanupSuggestion {
  scaffold_id: string;
  scope_id: string;
  label: string;
  desc: string;
  path: string;
  bytes: number;
  files: number;
  total_bytes: number;
}

export async function cleanupSuggestions(
  rootPath: string,
  _scopeDays?: Record<string, number>,
  _cachedOnly?: boolean,
): Promise<CleanupSuggestion[]> {
  await wait(60);
  // R225：与 scan 同源——总览建议从「剪过已回收路径的树」算，真删后
  // 建议项立即消失/变小，不再显示已清掉的目录。
  const tree = pruneRecycledFrom(structuredClone(MOCK_TREE));
  const hits = new Map<string, string>();
  collectScaffoldHits(tree, rootPath, hits);
  const out: CleanupSuggestion[] = [];
  for (const [scaffoldId, hitPath] of hits) {
    const sc = SCAFFOLDS.find((s) => s.id === scaffoldId);
    if (!sc) continue;
    const picked = sc.scopes
      .filter((s) => (s.category === 'cache' || s.category === 'media') && s.prompt?.kind !== 'confirm')
      .slice(0, 2);
    for (const scope of picked) {
      // R212：与清理页 scope 行同一口径——带 prompt.days 的 scope 用默认
      // 保留天数缩放（总览建议的大小 = 进清理页勾选后真正清的大小）。
      // R227：改真实命中字节（与清理页/工作台同源），不再用哈希公式。
      // R228：按 scope 的 glob 目录名取自己的真实字节。
      // R236：rootPath 是盘根（总览选中盘）时传回过滤——建议字节限定在
      // 该盘命中目录内，不含无关目录（多目录脚本各目录都算，合计不再虚高）。
      const days = scope.prompt?.kind === 'days' ? (scope.prompt.default ?? 30) : undefined;
      const real = rootPath.toLowerCase().replace(/[\\/]+$/, '') === 'c:'
        ? scopeRealBytes(sc.id, scope.id, scope.glob, rootPath)
        : scopeRealBytes(sc.id, scope.id, scope.glob);
      const raw = real ?? mockScopeBytes(scope.id);
      const bytes = days === undefined ? raw : Math.round(raw * Math.max(0.3, 1 - Math.min(days, 30) / 30 * 0.7));
      // R236：rootPath 过滤下该 scope 不在盘内（返回 0）→ 该建议项不算入，
      // 避免「0 字节建议」占位。
      if (bytes <= 0) continue;
      out.push({
        scaffold_id: sc.id,
        scope_id: scope.id,
        label: scope.label,
        desc: `${sc.name} · ${scope.category === 'media' ? '媒体' : '缓存'}`,
        path: hitPath,
        bytes,
        files: mockScopeFileCount(scope.id),
        total_bytes: bytes,
      });
    }
  }
  return out;
}

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86400;

export const STEAM_INVENTORY: SteamInventory = {
  steam_root: 'C:/Program Files (x86)/Steam',
  candidates_checked: [
    'C:/Program Files (x86)/Steam',
    'C:/Program Files/Steam',
  ],
  libraries: [
    {
      root: 'C:/Program Files (x86)/Steam',
      total_size_bytes: 80 * GB,
      residue: [],
      residue_size_bytes: 0,
      games: [
        {
          appid: 730,
          name_en: 'Counter-Strike 2',
          name_cn: null,
          install_dir_name: 'Counter-Strike Global Offensive',
          install_path: 'C:/Program Files (x86)/Steam/steamapps/common/Counter-Strike Global Offensive',
          appmanifest_path: 'C:/Program Files (x86)/Steam/steamapps/appmanifest_730.acf',
          size_bytes: 35 * GB,
          last_played_ts: NOW - DAY,
          last_updated_ts: NOW - 12 * DAY,
          bytes_to_download: 0,
          bytes_downloaded: 0,
          library_root: 'C:/Program Files (x86)/Steam',
          state_flags: 4,
          is_fully_installed: true,
          is_ghost: false,
          default_recommended: false,
          recommendation_reason: null,
          workshop_item_count: 7,
        },
        {
          appid: 440,
          name_en: 'Team Fortress 2',
          name_cn: null,
          install_dir_name: 'Team Fortress 2',
          install_path: 'C:/Program Files (x86)/Steam/steamapps/common/Team Fortress 2',
          appmanifest_path: 'C:/Program Files (x86)/Steam/steamapps/appmanifest_440.acf',
          size_bytes: 45 * GB,
          last_played_ts: NOW - 270 * DAY,
          last_updated_ts: NOW - 300 * DAY,
          bytes_to_download: 0,
          bytes_downloaded: 0,
          library_root: 'C:/Program Files (x86)/Steam',
          state_flags: 4,
          is_fully_installed: true,
          is_ghost: false,
          default_recommended: true,
          recommendation_reason: '45GB · 9 个月未启动',
          workshop_item_count: 0,
        },
      ],
    },
    {
      root: 'D:/SteamLibrary',
      total_size_bytes: 192 * GB,
      residue: [
        {
          kind: 'install',
          dir_name: 'Leftover Game (uninstalled)',
          path: 'D:/SteamLibrary/steamapps/common/Leftover Game (uninstalled)',
          size_bytes: 12 * GB,
          appid_hint: null,
          last_modified_ts: NOW - 210 * DAY,
        },
        {
          kind: 'workshop',
          dir_name: '999',
          path: 'D:/SteamLibrary/steamapps/workshop/content/999',
          size_bytes: 1.2 * GB,
          appid_hint: 999,
          last_modified_ts: NOW - 95 * DAY,
        },
      ],
      residue_size_bytes: 12 * GB + 1.2 * GB,
      games: [
        {
          appid: 1091500,
          name_en: 'Cyberpunk 2077',
          name_cn: null,
          install_dir_name: 'Cyberpunk 2077',
          install_path: 'D:/SteamLibrary/steamapps/common/Cyberpunk 2077',
          appmanifest_path: 'D:/SteamLibrary/steamapps/appmanifest_1091500.acf',
          size_bytes: 72 * GB,
          last_played_ts: null,
          last_updated_ts: NOW - 20 * DAY,
          bytes_to_download: 0,
          bytes_downloaded: 0,
          library_root: 'D:/SteamLibrary',
          state_flags: 4,
          is_fully_installed: true,
          is_ghost: false,
          default_recommended: true,
          recommendation_reason: '72GB · 从未启动',
          workshop_item_count: 0,
        },
        {
          appid: 1174180,
          name_en: 'Red Dead Redemption 2',
          name_cn: null,
          install_dir_name: 'Red Dead Redemption 2',
          install_path: 'D:/SteamLibrary/steamapps/common/Red Dead Redemption 2',
          appmanifest_path: 'D:/SteamLibrary/steamapps/appmanifest_1174180.acf',
          size_bytes: 119 * GB,
          last_played_ts: NOW - 540 * DAY,
          last_updated_ts: NOW - 5 * DAY,
          bytes_to_download: 0,
          bytes_downloaded: 0,
          library_root: 'D:/SteamLibrary',
          state_flags: 4,
          is_fully_installed: true,
          is_ghost: false,
          default_recommended: true,
          recommendation_reason: '119GB · 1 年未启动',
          workshop_item_count: 0,
        },
        {
          appid: 999,
          name_en: 'Forgotten Game',
          name_cn: null,
          install_dir_name: 'Forgotten Game',
          install_path: 'D:/SteamLibrary/steamapps/common/Forgotten Game',
          appmanifest_path: 'D:/SteamLibrary/steamapps/appmanifest_999.acf',
          size_bytes: 1 * GB,
          last_played_ts: null,
          last_updated_ts: NOW - 30 * DAY,
          bytes_to_download: 6 * GB,
          bytes_downloaded: 2 * GB,
          library_root: 'D:/SteamLibrary',
          state_flags: 4,
          is_fully_installed: false,
          is_ghost: true,
          default_recommended: true,
          recommendation_reason: 'ACF 存在但安装目录缺失',
          workshop_item_count: 0,
        },
      ],
    },
  ],
};

export function steamWorkshopItems(appid: number): WorkshopItem[] {
  if (appid !== 730) return [];
  return [
    {
      id: 2185699891,
      size_bytes: 145 * 1024 * 1024,
      last_modified_ts: NOW - 5 * DAY,
      path: `C:/Program Files (x86)/Steam/steamapps/workshop/content/${appid}/2185699891`,
    },
    {
      id: 3010055,
      size_bytes: 320 * 1024 * 1024,
      last_modified_ts: NOW - 90 * DAY,
      path: `C:/Program Files (x86)/Steam/steamapps/workshop/content/${appid}/3010055`,
    },
    {
      id: 3010099,
      size_bytes: 78 * 1024 * 1024,
      last_modified_ts: NOW - 400 * DAY,
      path: `C:/Program Files (x86)/Steam/steamapps/workshop/content/${appid}/3010099`,
    },
  ];
}

export function workshopTitles(ids: number[]): Record<number, string> {
  const knownTitles: Record<number, string> = {
    2185699891: 'CSGOHUB Skills Training Map by csstats.gg',
    3010055: 'Aim Practice Pack',
    // 3010099 intentionally omitted — simulates a deleted/private item
    // that Steam's API skips, so the UI shows the ID-only fallback path.
  };
  const out: Record<number, string> = {};
  for (const id of ids) {
    if (knownTitles[id]) out[id] = knownTitles[id];
  }
  return out;
}

// ── 浏览器预览用工具墙 mock（12 分类，与真实图吧工具箱目录结构一致）──
// 真实 Tauri 环境走后端 toolbelt_catalog；这里让预览模式也能看到工具墙全貌。
export const TOOLBELT_CATALOG = {
  tools_root: 'C:/mock/图吧工具箱/Tools',
  total: 18,
  category_order: ['处理器工具', '显卡工具', '硬盘工具', '综合检测', '其他工具', '烤鸡工具', '内存工具', '外设工具', '显示器工具', '主板工具', '游戏工具', '常用工具'],
  categories: [
    { name: '处理器工具', tools: [
      { name: 'CPU-Z', category: '处理器工具', description: 'CPU/主板/内存详细参数', exe_rel: 'CPU-Z/cpuz_x64.exe', arch: 'x64', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['检测'] },
      { name: 'Super π', category: '处理器工具', description: 'CPU 运算性能测试', exe_rel: 'SuperPI/super_pi.exe', arch: 'x86', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['跑分'] },
    ]},
    { name: '显卡工具', tools: [
      { name: 'GPU-Z', category: '显卡工具', description: '显卡详细参数/传感器', exe_rel: 'GPU-Z/gpuz.exe', arch: 'x64', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['检测'] },
      { name: 'FurMark', category: '显卡工具', description: '显卡烤机压力测试', exe_rel: 'FurMark/FurMark.exe', arch: 'x64', risk: 'medium', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['烤机', '压测'] },
    ]},
    { name: '硬盘工具', tools: [
      { name: 'CrystalDiskInfo', category: '硬盘工具', description: '硬盘健康/SMART 检测', exe_rel: 'HardDisk/CrystalDiskInfo/CrystalDiskInfo.exe', arch: 'x64', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: ['综合检测'], tags: ['健康', 'SMART'] },
      { name: 'WizTree', category: '硬盘工具', description: '磁盘占用分析（MFT 秒扫）', exe_rel: 'HardDisk/WizTree/WizTree.exe', arch: 'x64', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['占用分析'] },
      { name: 'CrystalDiskMark', category: '硬盘工具', description: '磁盘读写速度基准', exe_rel: 'HardDisk/CrystalDiskMark/CrystalDiskMark.exe', arch: 'x64', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['跑分'] },
    ]},
    { name: '综合检测', tools: [
      { name: 'AIDA64', category: '综合检测', description: '全方位硬件信息/稳定性测试', exe_rel: 'AIDA64/aida64.exe', arch: 'x64', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['全面'] },
      { name: 'HWiNFO', category: '综合检测', description: '实时传感器/详细硬件报告', exe_rel: 'AIDA64/../HWiNFO/HWiNFO64.exe', arch: 'x64', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['传感器'] },
    ]},
    { name: '其他工具', tools: [
      { name: 'Everything', category: '其他工具', description: 'NTFS 秒搜文件', exe_rel: 'Everything/Everything.exe', arch: 'x64', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['搜索'] },
    ]},
    { name: '烤鸡工具', tools: [
      { name: 'Prime95', category: '烤鸡工具', description: 'CPU 极限稳定性压力测试', exe_rel: 'BurnIn/Prime95/prime95.exe', arch: 'x64', risk: 'medium', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['压测'] },
      { name: 'AIDA64 稳定性测试', category: '烤鸡工具', description: 'FPU/内存/缓存综合烤机', exe_rel: 'BurnIn/AIDA64/aida64.exe', arch: 'x64', risk: 'medium', is_linked: false, is_builtin_link: false, linked_from: ['综合检测'], tags: ['压测'] },
    ]},
    { name: '内存工具', tools: [
      { name: 'MemTest', category: '内存工具', description: '内存错误检测', exe_rel: 'Memory/MemTest/MemTest.exe', arch: 'x86', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['检测'] },
    ]},
    { name: '外设工具', tools: [
      { name: 'TestPad', category: '外设工具', description: '键盘按键测试', exe_rel: 'Device/TestPad/TestPad.exe', arch: 'x86', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['键盘'] },
    ]},
    { name: '显示器工具', tools: [
      { name: 'DisplayX', category: '显示器工具', description: '显示器坏点/灰度测试', exe_rel: 'Screen/DisplayX/DisplayX.exe', arch: 'x86', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['坏点'] },
    ]},
    { name: '主板工具', tools: [
      { name: 'BIOS 信息', category: '主板工具', description: 'BIOS 版本/日期查看', exe_rel: 'MainBoard/BIOS/canary.exe', arch: 'x86', risk: 'low', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['BIOS'] },
    ]},
    { name: '游戏工具', tools: [
      { name: 'DX Repair', category: '游戏工具', description: 'DirectX 运行时修复', exe_rel: 'Game/DirectXRepair/DirectX_Repair.exe', arch: 'x86', risk: 'medium', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['DX'] },
    ]},
    { name: '常用工具', tools: [
      { name: 'UninstallTool', category: '常用工具', description: '深度卸载残留清理', exe_rel: 'Common/UninstallTool/UninstallTool.exe', arch: 'x64', risk: 'medium', is_linked: false, is_builtin_link: false, linked_from: [], tags: ['卸载'] },
    ]},
  ],
};
