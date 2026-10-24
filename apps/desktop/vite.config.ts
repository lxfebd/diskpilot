import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: '127.0.0.1',
    watch: { ignored: ['**/src-tauri/**'] },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: 'es2021',
    minify: 'esbuild',
    sourcemap: false,
    outDir: 'dist',
    rollupOptions: {
      output: {
        manualChunks: {
          // 框架/运行时独立成块：缓存友好 + 首屏只加载需要的部分
          'vendor-react': ['react', 'react-dom'],
          'vendor-tauri': ['@tauri-apps/api', '@tauri-apps/plugin-dialog', '@tauri-apps/plugin-fs', '@tauri-apps/plugin-shell'],
          'vendor-markdown': ['react-markdown', 'remark-gfm'],
          // 图标库单独成块：lucide-react 的按需 re-export 表跟着主 chunk 走会让
          // 首屏入口包压到 500 kB 以上（R336 前 537 kB），拆出来主包回落到阈值内。
          'vendor-icons': ['lucide-react'],
          // 浏览器预览（非 Tauri）用的假数据：165 kB 源码全量躺在主 chunk 里，
          // 桌面正式包永远走 invoke，不需要它首屏就位——单开一块，主包才能进阈值。
          'app-mocks': ['./src/mocks.ts'],
        },
      },
    },
  },
});
