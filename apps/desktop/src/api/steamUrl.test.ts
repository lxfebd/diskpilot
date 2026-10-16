// R202/R204：openSteamUrl 非 Tauri 分支的 URL 映射（steamUrlFor 纯函数）。
// workshop 走公开 https 版（浏览器能真开），uninstall/rungameid/validate 走
// steam:// 协议，nav 落商店页——不静默假成功，也不依赖 Tauri 环境。
import { describe, it, expect } from 'vitest';
import { steamUrlFor } from './system-ext';

describe('steamUrlFor（Steam 打开动作 URL 映射）', () => {
  it('workshop_page → steamcommunity 公开 https 版（浏览器真实可开）', () => {
    expect(steamUrlFor('workshop_page', 3010099)).toBe(
      'https://steamcommunity.com/sharedfiles/filedetails/?id=3010099',
    );
  });

  it('uninstall / rungameid / validate → steam:// 协议', () => {
    expect(steamUrlFor('uninstall', 730)).toBe('steam://uninstall/730');
    expect(steamUrlFor('rungameid', 730)).toBe('steam://rungameid/730');
    expect(steamUrlFor('validate', 730)).toBe('steam://validate/730');
  });

  it('nav → store.steampowered.com 商店页', () => {
    expect(steamUrlFor('nav', 730)).toBe('https://store.steampowered.com/app/730');
  });

  it('不把 appid 当字符串拼接（数字边界）', () => {
    expect(steamUrlFor('workshop_page', 0)).toBe(
      'https://steamcommunity.com/sharedfiles/filedetails/?id=0',
    );
    expect(steamUrlFor('nav', 999999999)).toBe(
      'https://store.steampowered.com/app/999999999',
    );
  });
});