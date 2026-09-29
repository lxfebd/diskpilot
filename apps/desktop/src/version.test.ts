// ── 版本号三处单源守卫 ─────────────────────────────────────────────
// 发布只改 apps/desktop/package.json 的 version，但 tauri.conf.json（产物
// 元数据）与根 Cargo.toml（workspace 版本，src-tauri crate 经
// version.workspace 继承）各持一份拷贝。这里断言三处一致，改漏任何一处
// 直接红，不再靠发布清单人肉记忆。
import { describe, expect, it } from 'vitest';
import pkg from '../package.json';
import tauriConf from '../src-tauri/tauri.conf.json';
import cargoRaw from '../../../Cargo.toml?raw';

const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargoRaw)?.[1];

describe('版本号单源', () => {
  it('package.json = tauri.conf.json = Cargo.toml(workspace)', () => {
    expect(pkg.version).toMatch(/^\d+\.\d+\.\d+$/);
    expect(tauriConf.version).toBe(pkg.version);
    expect(cargoVersion).toBe(pkg.version);
  });
});
