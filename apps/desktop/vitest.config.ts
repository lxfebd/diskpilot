import { defineConfig } from 'vitest/config';

// 纯函数单测：node 环境即可（store/theme/permissions 的 localStorage 访问
// 都在函数内部且 try/catch 兜底，模块顶层不碰浏览器 API）。
export default defineConfig({
  test: {
    environment: 'node',
    include: ['src/**/*.test.{ts,tsx}'],
    // _backup_spec/ 是历史高权限计划文件（勿执行），里面的旧 spec 已失效且
    // 依赖缺失，vitest 兜底扫描到会误报失败——显式排除，绝不动文件内容。
    exclude: ['**/_backup_spec/**', '**/node_modules/**', '**/dist/**'],
  },
});
