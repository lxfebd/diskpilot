import { describe, expect, it } from 'vitest';

const RAW = import.meta.glob('./components/StartupPanel.tsx', { query: '?raw', import: 'default', eager: true }) as Record<string, string>;

// R447：启动项开关两步确认的两个毛病——① 确认后（成功或失败）确认条不收：
// 列表重载后 enabled 已翻转，确认条文案变成「确认恢复？」这种反着问的提示，
// 看起来像刚才那一击没生效，还留着一个能再点一次的口子；② 在途双击会就同一项
// 发两次相反请求。这里把两处守卫钉住。
describe('启动项开关/删除的在途守卫（R447）', () => {
  const src = () => String(RAW['./components/StartupPanel.tsx'] ?? '');

  it('两个操作都有防重入锁，结束后收起各自的确认条', () => {
    const s = src();
    expect(s).toContain('const mutatingRef = useRef(false);');
    expect((s.match(/if \(mutatingRef\.current\) return;/g) || []).length).toBe(2);
    const toggle = s.slice(s.indexOf('const toggleStartup = async'), s.indexOf('const removeStartup = async'));
    expect(toggle).toContain('mutatingRef.current = true;');
    expect(toggle).toContain('setMutatingId(item.id);');
    expect(toggle).toContain('setPendingToggle(null);');
    const remove = s.slice(s.indexOf('const removeStartup = async'), s.indexOf('useEffect(() => {', s.indexOf('const removeStartup = async')));
    expect(remove).toContain('mutatingRef.current = true;');
    expect(remove).toContain('setPendingDel(null);');
  });

  it('在途时四个确认 / 取消按钮都禁用', () => {
    const s = src();
    expect((s.match(/disabled=\{mutatingId !== null\}/g) || []).length).toBe(4);
  });
});
