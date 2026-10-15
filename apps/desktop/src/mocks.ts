import type { Node, Scaffold, UndoEntry, SteamInventory, WorkshopItem } from './types';

const GB = 1024 ** 3;

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

const MOCK_TREE: Node = {
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
                  name: 'Local', path: 'C:\\Users\\90740\\AppData\\Local', is_dir: true, size: 51 * GB, file_count: 280_110,
                  scaffold_id: null, top_extensions: [],
                  children: [
                    leaf('Microsoft', 'C:\\Users\\90740\\AppData\\Local\\Microsoft', 14.8 * GB, 88_400, null),
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
                  ],
                },
                {
                  name: 'Roaming', path: 'C:\\Users\\90740\\AppData\\Roaming', is_dir: true, size: 17.7 * GB, file_count: 95_400,
                  scaffold_id: null, top_extensions: [], children: [
                    leaf('Tencent', 'C:\\Users\\90740\\AppData\\Roaming\\Tencent', 2.3 * GB, 4_100, null),
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
  return structuredClone(MOCK_TREE);
}

/** R182：预览模式的磁盘清单。真实桌面端从系统读盘（list_drives），浏览器
 *  里没有真盘——返回与 MOCK_TREE 同源的一块 C 盘（187.1GB 总量），让总览的
 *  磁盘详情、清理建议、空间趋势、救援横幅在演示时不再是「未检测到磁盘」。 */
export async function listDrives(): Promise<
  { total_bytes: number; used_bytes: number; free_bytes: number; path: string; mapped_to: string }[]
> {
  await wait(30);
  const total = 187.1 * GB;
  const used = 148.1 * GB;
  return [{ total_bytes: total, used_bytes: used, free_bytes: total - used, path: 'C:\\', mapped_to: '' }];
}

/** R183：预览模式的空间历史——总览「磁盘空间趋势」卡的演示数据。
 *  与 listDrives 同一块 C 盘：近 14 天每日一采样，已用空间缓慢爬升（含
 *  R181/R182 演示清理时释放的一截回落，趋势卡能看到「清完下降」的增量）。
 *  采样 > 5 点 → 趋势卡判定 rich（可信），增量与耗尽预测都能展示。 */
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
  pts[pts.length - 1] = { root: 'C:\\', total_bytes: total, used_bytes: 148.1 * GB, at: now };
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

/** 只读枚举：返回样例启动项副本（避免调用方改动污染模块级数组）。 */
export async function listStartupItems(): Promise<MockStartupItem[]> {
  await wait(60);
  return MOCK_STARTUP.map((it) => ({ ...it }));
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
  return [{ path: 'C:\\', days: 112, reason: 'ok', warn: false }];
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

function weeklyReportAt(ts: number): MockWeeklyReport {
  return {
    ts,
    drives: [{ path: 'C:\\', bytes: 2.91 * GB }],
    total_bytes: 2.91 * GB,
    temp_max_c: 52,
    hw_snapshots: 2,
    hw_delta: ['CPU 最高温 52°C（上周 55°C）', '硬盘通电累计 +168 小时'],
    disks: [{ friendly: 'Samsung SSD 980 PRO 1TB', letters: 'C', health: 'Healthy 100% (Samsung Magician 口径)', grade: 'good' }],
  };
}

const WEEKLY_ARCHIVE: MockWeeklyReport[] = [];

/** 生成一份新周报：写入演示归档（getWeeklyReports 立即能读到）。 */
export async function generateWeeklyReport(): Promise<MockWeeklyReport> {
  await wait(150);
  const now = Math.floor(Date.now() / 1000);
  const r = weeklyReportAt(now);
  WEEKLY_ARCHIVE.unshift(r);
  if (WEEKLY_ARCHIVE.length > 10) WEEKLY_ARCHIVE.pop();
  return r;
}

/** 最近 N 份周报（最新在前）。未生成过时先出一份上周的样例，让环比一开始可看。 */
export async function getWeeklyReports(limit?: number): Promise<MockWeeklyReport[]> {
  await wait(40);
  if (WEEKLY_ARCHIVE.length === 0) {
    const now = Math.floor(Date.now() / 1000);
    WEEKLY_ARCHIVE.push(weeklyReportAt(now - 7 * 86_400));
  }
  return WEEKLY_ARCHIVE.slice(0, limit ?? 5).map((r) => ({ ...r }));
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
  const entries = paths.map((src) => ({
    timestamp: new Date().toISOString(),
    action: 'recycle' as const,
    source: src,
    destination: null,
    reason,
  }));
  UNDO_LEDGER.push(...entries);
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

/** 只有隔离条目能原地还原（与后端 restore_quarantined 同口径）；回收条目返回 null。 */
export async function undo(index: number, source: string): Promise<UndoEntry | null> {
  await wait(300);
  const sorted = [...UNDO_LEDGER].sort((a, b) => b.timestamp.localeCompare(a.timestamp));
  const e = sorted[index];
  if (!e || e.action !== 'quarantine' || e.source !== source) return null;
  const at = UNDO_LEDGER.indexOf(e);
  if (at >= 0) UNDO_LEDGER.splice(at, 1);
  return e;
}

function wait(ms: number) { return new Promise<void>((r) => setTimeout(r, ms)); }

// —— 预览模式（非 Tauri）清理演示数据 ——
// 真实桌面端走后端 scope_sizes_batch / execute_scope；浏览器里没有磁盘可扫，
// 这些 mock 让「勾选 scope → 预览清单 → 真删 → 进度条/取消 → 历史台账」整条
// 链路在浏览器里可点可看（数字是演示数据，与 MOCK_TREE 的目录规模同量级）。
// 用 scope_id 做确定性哈希：同一 scope 每次返回同一批尺寸，界面刷新不跳变。

const MB = 1024 ** 2;

function hashString(s: string): number {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
}

/** scope 的演示体量：50MB–2.5GB，按 id 哈希稳定。 */
function mockScopeBytes(scopeId: string): number {
  const h = hashString(scopeId);
  return (30 + ((h % 220) / 100)) * 10 * MB * 0.5 + (h % 170) * MB;
}

function mockScopeFileCount(scopeId: string): number {
  return 40 + (hashString(scopeId + ':n') % 4000);
}

/** dry-run / 真删的演示命中条目：路径拼在扫描命中目录下，文件名按 scope 稳定。 */
function mockScopeEntries(scopeId: string, rootPath: string, n: number): UndoEntry[] {
  const bytes = mockScopeBytes(scopeId);
  const per = Math.max(1, Math.floor(bytes / Math.max(1, n)));
  const stem = hashString(scopeId + ':stem');
  const out: UndoEntry[] = [];
  for (let i = 0; i < n; i++) {
    out.push({
      timestamp: new Date().toISOString(),
      action: 'recycle' as const,
      source: `${rootPath}\\${scopeId}\\mock_${stem % 13}_chunk_${i + 1}`,
      reason: scopeId,
      bytes_freed: i === 0 ? bytes - per * (n - 1) : per,
    });
  }
  return out;
}

export async function scopeSizesBatch(
  scaffoldId: string,
  rootPaths: string[],
): Promise<{ root_path: string; sizes: { scope_id: string; bytes: number; file_count: number; total_bytes: number; total_files: number }[] }[]> {
  await wait(50);
  const scaffold = SCAFFOLDS.find((s) => s.id === scaffoldId);
  const sizes = (scaffold?.scopes ?? []).map((sc) => {
    const bytes = mockScopeBytes(sc.id);
    const files = mockScopeFileCount(sc.id);
    return {
      scope_id: sc.id,
      bytes,
      file_count: files,
      total_bytes: bytes,
      total_files: files,
    };
  });
  return rootPaths.map((p) => ({ root_path: p, sizes }));
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
  void opts;
  await wait(dryRun ? 60 : 120);
  const n = Math.min(64, Math.max(4, Math.ceil(mockScopeBytes(scopeId) / (24 * MB))));
  const entries = mockScopeEntries(scopeId, rootPath, n);
  if (!dryRun) {
    UNDO_LEDGER.push(...entries);
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
  const hits = new Map<string, string>();
  collectScaffoldHits(MOCK_TREE, rootPath, hits);
  const out: CleanupSuggestion[] = [];
  for (const [scaffoldId, hitPath] of hits) {
    const sc = SCAFFOLDS.find((s) => s.id === scaffoldId);
    if (!sc) continue;
    const picked = sc.scopes
      .filter((s) => (s.category === 'cache' || s.category === 'media') && s.prompt?.kind !== 'confirm')
      .slice(0, 2);
    for (const scope of picked) {
      out.push({
        scaffold_id: sc.id,
        scope_id: scope.id,
        label: scope.label,
        desc: `${sc.name} · ${scope.category === 'media' ? '媒体' : '缓存'}`,
        path: hitPath,
        bytes: mockScopeBytes(scope.id),
        files: mockScopeFileCount(scope.id),
        total_bytes: mockScopeBytes(scope.id),
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
