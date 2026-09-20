import { Component, ErrorInfo, ReactNode } from 'react';
// class 组件用不了 hook：在 render 里调用模块级 t()，每次重渲染现取文案，
// 不在模块顶层求值（那会把中文烤死在首次 import）。
import { t } from '../i18n';

interface Props { children: ReactNode; fallbackLabel?: string }
interface State { error: Error | null; info: ErrorInfo | null }

export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, info: null };

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[ErrorBoundary]', error, info);
    this.setState({ info });
  }

  reset = () => this.setState({ error: null, info: null });

  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div style={{ padding: 16, fontFamily: 'monospace', fontSize: 12, color: 'var(--err)', overflow: 'auto', maxHeight: '100%' }}>
        <div style={{ fontWeight: 600, marginBottom: 8 }}>
          {this.props.fallbackLabel ?? t('shell.boundary.fallback')}
        </div>
        <div style={{ marginBottom: 8 }}>{String(this.state.error.message ?? this.state.error)}</div>
        {this.state.error.stack && (
          <pre style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-all', background: 'var(--accent-bg)', padding: 8, borderRadius: 6 }}>
            {this.state.error.stack}
          </pre>
        )}
        <button onClick={this.reset} style={{ marginTop: 8 }}>{t('shell.boundary.reset')}</button>
      </div>
    );
  }
}
