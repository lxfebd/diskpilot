// 应用版本号的单一来源。
// 发布时只改 apps/desktop/package.json 的 version（它同时驱动 tauri.conf.json
// 的产物元数据），界面各处再从这里取值——避免文案表里手写 "v0.2.1" 之类的
// 字符串，随版本推进逐次腐烂成过期信息。
import pkg from '../package.json';

export const APP_VERSION: string = pkg.version;
