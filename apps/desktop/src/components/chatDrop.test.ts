// ── 聊天框非图片拖入的反馈守卫（R450）──────────────────────────────
// 聊天框只收图片，但「选择文件」对话框能切到所有文件、拖入也可能混进 PDF/zip。
// 以前这些文件被静默丢弃：没有药丸、没有提示、也没有系统消息——用户以为卡住了。
import { describe, it, expect } from 'vitest';

const RAW = import.meta.glob('./ChatPanel.tsx', {
  query: '?raw',
  import: 'default',
  eager: true,
}) as Record<string, string>;

const src = RAW['./ChatPanel.tsx'];

describe('聊天框非图片拖入的反馈（R450）', () => {
  it('非图片文件走统一提示，不再静默 return', () => {
    expect(src).toContain('chat.sys.imageNotImage');
    expect(src).toContain('const skipNonImages = (files: File[]) => {');
    expect(src).toContain("if (!file.type.startsWith('image/')) {");
    expect(src).toContain('skipNonImages([file]);');
    expect(src).not.toContain("if (!file.type.startsWith('image/')) return;");
  });

  it('拖入按图片 / 非图片分流：图片照收，其余一并说明', () => {
    expect(src).toContain('const dropped = Array.from(e.dataTransfer.files ?? []);');
    expect(src).toContain("for (const f of dropped.filter((f) => f.type.startsWith('image/'))) await addImageFile(f);");
    expect(src).toContain("skipNonImages(dropped.filter((f) => !f.type.startsWith('image/')));");
    expect(src).toContain('names =');
  });

  it('名称列表最多三条，避免一屏系统消息', () => {
    expect(src).toContain('files.slice(0, 3)');
    expect(src).toContain('for (const f of files) await addImageFile(f);');
  });
});
