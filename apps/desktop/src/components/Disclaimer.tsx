import { useEffect, useRef, useState } from 'react';
import { ShieldAlert, Github, Mail, MessageCircle, User } from 'lucide-react';
import { useT } from '../i18n';

/** 开屏免责声明弹窗：首次启动强制显示。
 *  - 10 秒倒计时未走完时「同意」不可点（保证用户读够时间）
 *  - 必须把声明滚到最底部才能点亮「同意」（保证声明被实际阅读）
 *  - 同意后写 localStorage，下次启动不再弹
 * 作者信息 / GitHub / 免责与责任边界全部以显眼方式呈现，纯前端展示。
 */
const DISCLAIMER_KEY = 'diskpilot.disclaimer.v1';

export function hasAcceptedDisclaimer(): boolean {
  try {
    return localStorage.getItem(DISCLAIMER_KEY) === '1';
  } catch {
    return false;
  }
}

const MIN_READ_SECONDS = 10;

/** 同意闸门：倒计时走完（读够时间）且已滚到底（声明被实际阅读）双条件同时满足才可点同意。
 * 抽成纯函数便于单测锁定——这道 10 秒强制门不能因后续改动被误放宽或误删。 */
export function canAcceptDisclaimer(countdown: number, reachedEnd: boolean): boolean {
  return countdown === 0 && reachedEnd;
}

/**
 * 把文案里的 `**粗体**` 片段渲染成 `<strong>`。
 * 免责条款原本是「句内带强调的整段话」，一键一句时靠这个标记保住强调，
 * 两种语言写在同一处、不用为每个强调片段拆键。
 */
function rich(s: string) {
  return s.split('**').map((part, i) => (i % 2 ? <strong key={i}>{part}</strong> : part));
}

export default function Disclaimer({ onAccept }: { onAccept?: () => void }) {
  const t = useT();
  const [countdown, setCountdown] = useState(MIN_READ_SECONDS);
  const [reachedEnd, setReachedEnd] = useState(false);
  const [open, setOpen] = useState(() => !hasAcceptedDisclaimer());
  const scrollRef = useRef<HTMLDivElement | null>(null);

  // 倒计时：只在弹窗打开时跑，走完归 0 停表（同意按钮解锁的第一条件）
  useEffect(() => {
    if (!open || countdown <= 0) return;
    const timer = window.setInterval(() => {
      setCountdown((c) => Math.max(0, c - 1));
    }, 1000);
    return () => window.clearInterval(timer);
  }, [open, countdown]);

  // 滚到底部判定：scrollTop + clientHeight >= scrollHeight - 容差
  const onScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    if (el.scrollTop + el.clientHeight >= el.scrollHeight - 8) {
      setReachedEnd(true);
    }
  };

  const accepted = canAcceptDisclaimer(countdown, reachedEnd);

  const accept = () => {
    if (!accepted) return;
    try {
      localStorage.setItem(DISCLAIMER_KEY, '1');
    } catch {
      /* 存储不可用则本次会话内不再弹 */
    }
    setOpen(false);
    onAccept?.();
  };

  if (!open) return null;

  return (
    <div className="disclaimer-mask">
      <div className="disclaimer-modal" role="dialog" aria-modal="true" aria-label={t('shell.disclaimer.ariaLabel')}>
        <div className="disclaimer-head">
          <ShieldAlert size={20} />
          <div>
            <h2>{t('shell.disclaimer.title')}</h2>
            <p className="disclaimer-sub">{t('shell.disclaimer.sub')}</p>
          </div>
        </div>

        <div className="disclaimer-body" ref={scrollRef} onScroll={onScroll}>
          <section className="disclaimer-author">
            <h3>
              <User size={14} /> {t('shell.disclaimer.authorHeading')}
            </h3>
            <ul>
              <li><b>{t('shell.disclaimer.authorLabel')}</b>{t('shell.disclaimer.authorName')}</li>
              <li><b>{t('shell.disclaimer.qqLabel')}</b>3167245951</li>
              <li><b>{t('shell.disclaimer.emailLabel')}</b><a href="mailto:3167245951@qq.com">3167245951@qq.com</a></li>
              <li><b>{t('shell.disclaimer.githubLabel')}</b><a href="https://github.com/lxfebd" target="_blank" rel="noreferrer">https://github.com/lxfebd</a></li>
            </ul>
          </section>

          <section>
            <h3>{t('shell.disclaimer.h1')}</h3>
            <p>{t('shell.disclaimer.p1')}</p>
          </section>

          <section>
            <h3>{t('shell.disclaimer.h2')}</h3>
            <p>{rich(t('shell.disclaimer.p2'))}</p>
          </section>

          <section>
            <h3>{t('shell.disclaimer.h3')}</h3>
            <p>{t('shell.disclaimer.p3')}</p>
            <ul>
              <li>{rich(t('shell.disclaimer.p3li1'))}</li>
              <li>{rich(t('shell.disclaimer.p3li2'))}</li>
              <li>{rich(t('shell.disclaimer.p3li3'))}</li>
              <li>{t('shell.disclaimer.p3li4')}</li>
            </ul>
          </section>

          <section>
            <h3>{t('shell.disclaimer.h4')}</h3>
            <p>{rich(t('shell.disclaimer.p4'))}</p>
            <ul>
              <li>{t('shell.disclaimer.p4li1')}</li>
              <li>{t('shell.disclaimer.p4li2')}</li>
              <li>{t('shell.disclaimer.p4li3')}</li>
              <li>{t('shell.disclaimer.p4li4')}</li>
              <li>{t('shell.disclaimer.p4li5')}</li>
            </ul>
            <p>{rich(t('shell.disclaimer.p4close'))}</p>
          </section>

          <section>
            <h3>{t('shell.disclaimer.h5')}</h3>
            <p>{t('shell.disclaimer.p5')}</p>
          </section>
        </div>

        <div className="disclaimer-foot">
          <div className="disclaimer-contact">
            <span className="chip"><User size={12} /> {t('shell.disclaimer.authorName')}</span>
            <span className="chip"><MessageCircle size={12} /> QQ 3167245951</span>
            <span className="chip"><Mail size={12} /> 3167245951@qq.com</span>
            <span className="chip"><Github size={12} /> github.com/lxfebd</span>
          </div>
          <button
            className="primary disclaimer-accept"
            disabled={!accepted}
            onClick={accept}
            title={accepted ? t('shell.disclaimer.tipAgreed') : reachedEnd ? t('shell.disclaimer.tipReadMore', { s: countdown }) : t('shell.disclaimer.tipScroll')}
          >
            {countdown > 0
              ? t('shell.disclaimer.countdown', { s: countdown })
              : reachedEnd
                ? t('shell.disclaimer.accept')
                : t('shell.disclaimer.scrollFirst')}
          </button>
        </div>
      </div>
    </div>
  );
}