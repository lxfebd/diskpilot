// ── GPU 甜甜圈压测（对标 FurMark 甜甜圈，WebGL2 片元着色器全屏渲染压真实 GPU）──
// 真正的 GPU 满载：片元着色器在每个像素上做海量浮点迭代——甜甜圈 SDF + 背景
// Mandelbrot/Julia/Burning Ship 分形 + 球体场 + 皮毛层，另带 4-pass 后处理链。
// 全屏渲染 + 真实 FPS 帧差统计 + 每秒轮询 hw_temperature 做 90℃ 熔断守门，Esc 退出。
// G13 显示器坏点检测（全屏纯色覆盖）也在此。
import { useCallback, useEffect, useRef, useState } from 'react';
import { X } from 'lucide-react';
import { api } from '../../api';
import { useT } from '../../i18n';
import type { TFunc } from './shared';

function deadPixelColors(t: TFunc): { name: string; value: string }[] {
  return [
    { name: t('toolbelt.dead.colorWhite'), value: '#ffffff' },
    { name: t('toolbelt.dead.colorBlack'), value: '#000000' },
    { name: t('toolbelt.dead.colorRed'), value: '#ff0000' },
    { name: t('toolbelt.dead.colorGreen'), value: '#00ff00' },
    { name: t('toolbelt.dead.colorBlue'), value: '#0000ff' },
    { name: t('toolbelt.dead.colorCyan'), value: '#00ffff' },
    { name: t('toolbelt.dead.colorMagenta'), value: '#ff00ff' },
    { name: t('toolbelt.dead.colorYellow'), value: '#ffff00' },
  ];
}

export function DeadPixelOverlay({ onExit }: { onExit: () => void }) {
  const t = useT();
  const [idx, setIdx] = useState(0);
  const colors = deadPixelColors(t);
  const color = colors[idx % colors.length];

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onExit();
      else if (e.key === ' ') {
        e.preventDefault();
        setIdx((i) => i + 1);
      }
    };
    window.addEventListener('keydown', onKey);
    document.body.style.overflow = 'hidden';
    return () => {
      window.removeEventListener('keydown', onKey);
      document.body.style.overflow = '';
    };
  }, [onExit]);

  return (
    <div
      className="dead-pixel-overlay"
      style={{ background: color.value }}
      onClick={() => setIdx((i) => i + 1)}
      onDoubleClick={onExit}
      title={t('toolbelt.dead.hintTitle')}
    >
      <div className="dead-pixel-hint" style={{ color: idx === 1 ? '#fff' : idx === 0 ? '#000' : '#000' }}>
        <span>{color.name}</span>
        <span className="muted small">{t('toolbelt.dead.hint')}</span>
      </div>
    </div>
  );
}

/// 从 `hw_temperature` 文本解析 GPU 温度：N 卡行「GPU（NVIDIA）：xx ℃」、
/// A 卡行「GPU（AMD）：xx ℃」任一命中返回数值；无温度行返回 null。
/// 这是甜甜圈 90℃ 熔断守门的唯一数据源，模块级导出以便单测锁定契约
/// （格式变更必须同步 agent-server hw.rs 输出与 `parse_gpu_temp_line` 测试）。
export function parseGpuTemp(text: string): number | null {
  // 匹配用的正则（后端中文输出格式），不是文案，保持原样。
  const m = text.match(/GPU[（(](?:NVIDIA|AMD)[）)]：\s*([\d.]+)\s*℃/); // @i18n-keep
  return m ? Number(m[1]) : null;
}

interface GpuDonutProps {
  onDone: (summary: string, canceled: boolean) => void;
}

export function GpuDonut({ onDone }: GpuDonutProps) {
  const t = useT();
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [stats, setStats] = useState<{ fps: number; secs: number; temp: string }>({
    fps: 0,
    secs: 0,
    temp: 'N/A',
  });
  const doneRef = useRef(false);
  // 真实帧计数：frame() 每渲染一帧 +1，每秒的 statTimer 读取差值为 FPS
  // （此前用 `Math.round(60 + Math.random() * 30)` 的假数，用户点名不可信；
  // 改为帧差统计，与 FurMark 的 FPS 口径一致）。
  const frames = useRef(0);
  // 挂载时刻：退出摘要算「平均 FPS = 总帧数 / 时长」
  const startRef = useRef(Date.now());
  // fpsRef：避免 setStats 触发 useEffect 依赖重启（shader 只编译一次）
  const fpsRef = useRef(60);

  const finish = useCallback(
    (canceled: boolean, summary?: string) => {
      if (doneRef.current) return;
      doneRef.current = true;
      // 退出摘要带真实统计：时长 + 平均 FPS（帧差口径，FurMark 同款）
      const secs = Math.max(1, Math.round((Date.now() - startRef.current) / 1000));
      const avgFps = Math.round(frames.current / secs);
      const base = t('toolbelt.bench.donut.summary', {
        secs,
        avgFps,
        manual: canceled ? t('toolbelt.bench.donut.manualTag') : '',
      });
      onDone(summary ?? base, canceled);
    },
    [onDone, t],
  );

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    // 全屏 + 高分屏适配（canvas 像素缓冲尺寸 = CSS 尺寸 × devicePixelRatio，
    // 让片元着色器跑在原生分辨率上，压力更真实）。
    // 注：压测密度由固定 4× 超采样分辨率驱动（~4K 全屏：6838×2400），
    // dpr 上限拉满让 fillrate 成为主瓶颈——这是 GPU 满载率独立于显示器刷新率、
    // 跑分可对齐 FurMark 的关键。
    // ⚠️ 实测教训（2026-09-16）：不能无限提分辨率到 2.5×（4800×3000）——
    // FBO 读写带宽成为瓶颈，GPU 大量时间卡在 memory stall，SM 浮点阵列空转，
    // 功耗反而从 472W 暴跌到 250W（nvidia-smi 把 stall 也算 busy，占用仍 99%）。
    // FurMark 是计算受限不是带宽受限：分辨率维持 2× 甜点位，靠提高
    // 每像素纯 FMA 密度（多层分形/皮毛/球体场）逼近功耗墙。
    const SCALE = 2; // 固定 2× 超采样 → 面积 ×4（带宽不瓶颈的甜点位）
    const resize = () => {
      const dpr = Math.min(2, window.devicePixelRatio || 1);
      canvas.width = Math.round(window.innerWidth * dpr * SCALE);
      canvas.height = Math.round(window.innerHeight * dpr * SCALE);
    };
    resize();
    window.addEventListener('resize', resize);
    document.body.style.overflow = 'hidden';

    // ── WebGL2 初始化：全屏四边形 + 片元着色器（GPU 每像素海量浮点迭代）──
    // desynchronized: 让 WebGL 交换缓冲区脱离显示器刷新率同步——压测必须让 GPU
    // 满速推帧（FurMark 从不等 vsync），否则 60Hz 显示器会把提交钳到 16.7ms 一拍，
    // GPU 画完 4 帧就空转等下一拍，占用/功耗被刷新率锁死。
    const gl = canvas.getContext('webgl2', {
      antialias: false,
      alpha: false,
      powerPreference: 'high-performance',
      desynchronized: true,
    });
    if (!gl) {
      onDone(t('toolbelt.donut.noWebgl2'), false);
      return;
    }
    const vsSrc = `#version 300 es
      layout(location = 0) in vec2 aPos;
      out vec2 vUv;
      void main() {
        vUv = aPos * 0.5 + 0.5;
        gl_Position = vec4(aPos, 0.0, 1.0);
      }
    `;
    // 片元着色器：每像素做 4 层重计算，纯 GPU 浮点压力（对标 FurMark shader）
    const fsSrc = `#version 300 es
      precision highp float;
      in vec2 vUv;
      out vec4 fragColor;
      uniform vec2 uRes;      // canvas 像素尺寸
      uniform float uTime;    // 秒（旋转/动画）
      uniform float uIter;    // 分形迭代上限（可调负载）

      // 甜甜圈 SDF：环面距离函数
      float sdTorus(vec3 p, float R, float r) {
        vec2 q = vec2(length(p.xz) - R, p.y);
        return length(q) - r;
      }
      // 旋转矩阵（绕 X / Y）
      mat3 rotX(float a) {
        float c = cos(a), s = sin(a);
        // 列主序：第一列 (1,0,0)，第二列 (0,c,s)，第三列 (0,-s,c)
        return mat3(1.0, 0.0, 0.0,   0.0, c, s,   0.0, -s, c);
      }
      mat3 rotY(float a) {
        float c = cos(a), s = sin(a);
        // 列主序：第一列 (c,0,-s)，第二列 (0,1,0)，第三列 (s,0,c)
        return mat3(c, 0.0, -s,   0.0, 1.0, 0.0,   s, 0.0, c);
      }
      // Mandelbrot 分形迭代：每像素最多 uIter 次复数平方，GPU 浮点压力主体。
      // 逃逸半径放宽到 256（FurMark 同哲学：让绝大多数像素跑满 uIter，
      // 每像素负载恒定，而不是大部分像素几步就 early-exit 空转）
      float mandelbrot(vec2 c) {
        vec2 z = vec2(0.0);
        float it = 0.0;
        for (int i = 0; i < 512; i++) {
          if (float(i) >= uIter) break;
          z = vec2(z.x*z.x - z.y*z.y, 2.0*z.x*z.y) + c;
          if (dot(z, z) > 256.0) break;
          it = float(i);
        }
        return it / uIter;
      }
            // Julia 集迭代：第二层分形，每像素再叠一轮复数迭代（负载翻倍）
      float julia(vec2 z, vec2 c) {
        float it = 0.0;
        for (int i = 0; i < 256; i++) {
          if (float(i) >= uIter * 0.5) break;
          z = vec2(z.x*z.x - z.y*z.y, 2.0*z.x*z.y) + c;
          if (dot(z, z) > 256.0) break;
          it = float(i);
        }
        return it / (uIter * 0.5);
      }
      // Burning Ship 分形：第三层纯 FMA 复数迭代（取绝对值破坏对称性，
      // 每像素再叠一轮高迭代运算——SM 浮点阵列压力再翻倍）
      float burningShip(vec2 c) {
        vec2 z = vec2(0.0);
        float it = 0.0;
        for (int i = 0; i < 256; i++) {
          if (float(i) >= uIter * 0.5) break;
          z = vec2(abs(z.x*z.x - z.y*z.y), abs(2.0*z.x*z.y)) + c;
          if (dot(z, z) > 256.0) break;
          it = float(i);
        }
        return it / (uIter * 0.5);
      }
            // 高迭代球体场（纯 FMA 浮点压力）：沿视线步进 48 层球谐叠加——
      // 全部是乘加运算，不走纹理/SFU，把 SM 的 FMA 阵列吃满（功耗跟着走）。
      float sphereField(vec3 ro, vec3 rd) {
        float acc = 0.0;
        vec3 p = ro;
        for (int i = 0; i < 48; i++) {
          p += rd * 0.02;
          float d = length(p - vec3(0.0, 0.0, 0.6)) * 0.5 +
                    length(p - vec3(0.4, 0.3, 0.2)) * 0.25 +
                    length(p - vec3(-0.4, -0.3, -0.2)) * 0.25;
          acc += smoothstep(0.5, 0.2, d);
        }
        return acc / 48.0;
      }

      // 全场毛密度（对齐 FurMark 皮毛材质）：沿视线对每个像素均匀步进 36 层
      // 3D 噪波，阈值化后叠加——整个屏幕都吃满采样，不再只作用于甜甜圈命中区。
      // 注意：hash 用纯算术（FMA），不用 sin()——sin 走 SFU 单元吞吐只有 FMA 的
      // 1/16，会把 SM 浮点阵列饿死；纯 FMA hash 让功耗跟着 SM 走。
      float hash31(vec3 p) {
        p = fract(p * 0.3183099 + vec3(0.1, 0.2, 0.3));
        p *= 17.0;
        return fract(p.x * p.y * p.z);
      }
      float globalFur(vec3 ro, vec3 rd, float t0) {
        float acc = 0.0;
        for (int i = 0; i < 36; i++) {
          vec3 q = ro + rd * (t0 + 0.05 * float(i));
          float n = hash31(q);
          acc += smoothstep(0.62, 0.92, n);
        }
        return acc / 36.0;
      }

// ── 皮毛渲染（对齐 FurMark 手法）──
    // 表层命中后不 break，沿法线向外做 20 层噪声采样：FurMark 的"皮毛"其实
    // 就是每像素 20-30 次厚度方向 SDF 采样 + 噪波抖动，负载比单次命中高一个数量级。
    float furLayered(vec3 pos, vec3 n, float R, float tubeR) {
      float acc = 0.0;
      for (int i = 0; i < 20; i++) {
        vec3 pF = pos + n * (0.001 + 0.006 * float(i));
        // 毛层抖动：纯算术 hash（FMA），模拟皮毛密度
        float h1 = hash31(pF);
        float h2 = hash31(pF + 3.1);
        vec3 jitter = n * 0.002 * (h1 - 0.5);
        float d2 = sdTorus(pF + jitter, R, tubeR);
        acc += smoothstep(0.004, 0.0, d2 + 0.004);
        acc += 0.7 * h2; // 毛色噪声：越靠近表面越亮
      }
      return acc / 20.0; // 0..1 毛密度
    }

    void main() {
      vec2 uv = vUv;
      vec2 p = (uv * uRes - uRes * 0.5) / min(uRes.x, uRes.y); // 归一化坐标
      // 背景：Mandelbrot + Julia 双层分形（每像素两轮复数迭代，GPU 压力主体）
      vec2 c0 = p * 1.1 + vec2(cos(uTime*0.3)*0.35, sin(uTime*0.2)*0.25) - vec2(0.45);
      float m1 = mandelbrot(c0);
      float j1 = julia(p * 0.9 + vec2(0.2), vec2(cos(uTime*0.7)*0.3, sin(uTime*0.5)*0.3));
      float bs = burningShip(p * 1.3 + vec2(0.1));
      float mb = clamp(m1 * 0.5 + j1 * 0.25 + bs * 0.25, 0.0, 1.0);
      vec3 bg = mix(vec3(0.02,0.01,0.05), vec3(0.10,0.05,0.18), mb);

      // 甜甜圈：旋转 + SDF 光线步进（48 步，法线差分光照 + 皮毛层）
      vec3 ro = vec3(0.0, 0.0, 2.6);
      vec3 rd = normalize(vec3(p, -1.5));
      mat3 rot = rotY(uTime * 0.9) * rotX(uTime * 0.55);
      float R = 0.72, tubeR = 0.30;
      float t = 0.0;
      vec3 col = bg;
      float fuzz = 0.0; // 皮毛命中密度，供光晕 pass 提亮
      for (int i = 0; i < 48; i++) {
        vec3 pos = rot * (ro + rd * t);
        float d = sdTorus(pos, R, tubeR);
        if (d < 0.0005) {
          vec2 e = vec2(0.001, 0.0);
          vec3 n = normalize(vec3(
            sdTorus(pos + e.xyy, R, tubeR) - sdTorus(pos - e.xyy, R, tubeR),
            sdTorus(pos + e.yxy, R, tubeR) - sdTorus(pos - e.yxy, R, tubeR),
            sdTorus(pos + e.yyx, R, tubeR) - sdTorus(pos - e.yyx, R, tubeR)
          ));
          float diff = max(dot(n, normalize(vec3(0.6, 0.7, 0.5))), 0.0);
          float spec = pow(max(dot(normalize(vec3(0.6,0.7,0.5)), normalize(reflect(rd, n))), 0.0), 24.0);
          // 核心色随温度/时间闪烁
          vec3 core = vec3(1.0, 0.35, 0.12) + 0.4 * sin(uTime * 3.0 + uv.x * 20.0);
          // 皮毛层：20 次厚度采样叠成毛色
          float fur = furLayered(pos, n, R, tubeR);
          fuzz = fur;
          col = mix(core * (0.25 + 0.75 * diff) + vec3(1.0) * spec * 0.8,
                    vec3(1.0, 0.9, 0.55) * fur * (0.5 + 0.5 * diff),
                    clamp(fur * 2.0, 0.0, 1.0));
          break;
        }
          t += d;
          if (t > 6.0) break;
        }

        // 全场毛密度场（每个像素都采 36 层，负载恒定，不依赖是否命中甜甜圈）
        float fg = globalFur(ro, rd, 0.4);
        // 毛色沿视线方向明暗渐变（近亮远暗，模拟皮毛厚度）
        vec3 furColor = vec3(1.0, 0.62, 0.20) * fg * 1.6;
        // 高迭代球体场（48 层纯 FMA）：与皮毛/分形叠加成混合负载，SM 浮点阵列吃满
        float sf = sphereField(ro, rd);
        vec3 sfCol = vec3(0.15, 0.25, 0.45) * sf * 1.8;

        // 粒子/星空：hash 采样 + 动态闪烁（叠加层，仍消耗每像素计算）
        vec3 stars = vec3(0.0);
        vec2 suv = uv * vec2(uRes.x/uRes.y, 1.0) * 3.0;
        vec2 g = floor(suv);
        float h = fract(sin(dot(g, vec2(12.9898, 78.233))) * 43758.5453);
        if (h > 0.985) stars = vec3(1.0) * (0.4 + 0.6 * sin(uTime * 4.0 + h * 60.0));

        fragColor = vec4(col + furColor + sfCol + stars * 0.7, 1.0);
      }
    `;
    const compile = (type: number, src: string): WebGLShader => {
      const sh = gl.createShader(type)!;
      gl.shaderSource(sh, src);
      gl.compileShader(sh);
      if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) {
        const info = gl.getShaderInfoLog(sh);
        gl.deleteShader(sh);
        throw new Error(t('toolbelt.donut.shaderCompileFailed', { info: String(info) }));
      }
      return sh;
    };
    const prog = gl.createProgram()!;
    const vs = compile(gl.VERTEX_SHADER, vsSrc);
    const fs = compile(gl.FRAGMENT_SHADER, fsSrc);
    gl.attachShader(prog, vs);
    gl.attachShader(prog, fs);
    gl.linkProgram(prog);
    if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
      onDone(t('toolbelt.donut.linkFailed', { info: String(gl.getProgramInfoLog(prog)) }), false);
      return;
    }
    gl.useProgram(prog);

    // 全屏四边形（两个三角形盖满视口）
    const vbo = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, vbo);
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1,-1, 1,-1, -1,1, -1,1, 1,-1, 1,1]), gl.STATIC_DRAW);
    const aPos = gl.getAttribLocation(prog, 'aPos');
    gl.enableVertexAttribArray(aPos);
    gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
    gl.viewport(0, 0, canvas.width, canvas.height);

    const uRes = gl.getUniformLocation(prog, 'uRes');
    const uTime = gl.getUniformLocation(prog, 'uTime');
    const uIter = gl.getUniformLocation(prog, 'uIter');

    // ── 后处理多 pass 链（每帧 4 个额外全屏 pass，让 GPU 持续满载）──
    // 管线：主场景(prog) → 高亮提取 → 水平模糊 → 垂直模糊 → 合成回屏幕。
    // 每 pass 都是全屏片元着色器，RTX 5090 单 shader 0.44ms 太轻，
    // 4 pass 叠加后才接近 FurMark 的 GPU 工作密度。
    const mkProgram = (fsSrc2: string): WebGLProgram => {
      const p = gl.createProgram()!;
      const v2 = compile(gl.VERTEX_SHADER, vsSrc);
      const f2 = compile(gl.FRAGMENT_SHADER, fsSrc2);
      gl.attachShader(p, v2);
      gl.attachShader(p, f2);
      gl.linkProgram(p);
      if (!gl.getProgramParameter(p, gl.LINK_STATUS)) {
        throw new Error(t('toolbelt.donut.postLinkFailed', { info: String(gl.getProgramInfoLog(p)) }));
      }
      return p;
    };
    // 高亮提取：亮部留下（甜甜圈高光 → 光晕源），暗处归零
    const fsBright = `#version 300 es
      precision highp float;
      in vec2 vUv;
      out vec4 fragColor;
      uniform sampler2D uTex;
      void main() {
        vec3 c = texture(uTex, vUv).rgb;
        float l = dot(c, vec3(0.299, 0.587, 0.114));
        fragColor = vec4(c * smoothstep(0.55, 0.9, l), 1.0);
      }
    `;
    // 高斯模糊（水平/垂直共用，uDir 选方向；13-tap 双 pass：带宽让位给 SM 浮点，
    // 与分形/皮毛形成「带宽+浮点」混合负载。实测 25-tap 会让带宽成瓶颈、SM 空转，
    // 功耗反降——5090 上 13-tap 是带宽/浮点的甜点位，SM 满载功耗最高。）
    const fsBlur = `#version 300 es
      precision highp float;
      in vec2 vUv;
      out vec4 fragColor;
      uniform sampler2D uTex;
      uniform vec2 uDir;
      uniform vec2 uRes;
      void main() {
        vec2 px = uDir / uRes;
        vec3 acc = vec3(0.0);
        // 13-tap 高斯权（和 ≈1.0）
        acc += texture(uTex, vUv - 6.0 * px).rgb * 0.0182;
        acc += texture(uTex, vUv - 5.0 * px).rgb * 0.0335;
        acc += texture(uTex, vUv - 4.0 * px).rgb * 0.0550;
        acc += texture(uTex, vUv - 3.0 * px).rgb * 0.0806;
        acc += texture(uTex, vUv - 2.0 * px).rgb * 0.1054;
        acc += texture(uTex, vUv - 1.0 * px).rgb * 0.1229;
        acc += texture(uTex, vUv).rgb * 0.1280;
        acc += texture(uTex, vUv + 1.0 * px).rgb * 0.1229;
        acc += texture(uTex, vUv + 2.0 * px).rgb * 0.1054;
        acc += texture(uTex, vUv + 3.0 * px).rgb * 0.0806;
        acc += texture(uTex, vUv + 4.0 * px).rgb * 0.0550;
        acc += texture(uTex, vUv + 5.0 * px).rgb * 0.0335;
        acc += texture(uTex, vUv + 6.0 * px).rgb * 0.0182;
        fragColor = vec4(acc, 1.0);
      }
    `;
    // 合成：主场景 + 光晕
    const fsComposite = `#version 300 es
      precision highp float;
      in vec2 vUv;
      out vec4 fragColor;
      uniform sampler2D uMain;
      uniform sampler2D uGlow;
      void main() {
        vec3 main = texture(uMain, vUv).rgb;
        vec3 glow = texture(uGlow, vUv).rgb;
        fragColor = vec4(main + glow * 0.7, 1.0);
      }
    `;
    const progBright = mkProgram(fsBright);
    const progBlurH = mkProgram(fsBlur);
    const progBlurV = mkProgram(fsBlur);
    const progComp = mkProgram(fsComposite);

    // FBO 三件套：主场景 / 高亮稿 / 模糊稿（转 ping-pong）
    const mkFbo = (w: number, h: number) => {
      const fb = gl.createFramebuffer()!;
      const tex = gl.createTexture()!;
      gl.bindTexture(gl.TEXTURE_2D, tex);
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA8, w, h, 0, gl.RGBA, gl.UNSIGNED_BYTE, null);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
      gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex, 0);
      if (gl.checkFramebufferStatus(gl.FRAMEBUFFER) !== gl.FRAMEBUFFER_COMPLETE) {
        throw new Error(t('toolbelt.donut.fboIncomplete'));
      }
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      return { fb, tex };
    };
    // 双缓冲 FBO（每帧用 k%2 选集）：帧间无 FBO 读写依赖，GPU 可跨帧流水线并行，
    // 消除"帧 N 写完才能画帧 N+1"的串行化——这是单缓冲时占用上不去的第二主因。
    const mkSet = (): { main: ReturnType<typeof mkFbo>; glow: ReturnType<typeof mkFbo>; blurA: ReturnType<typeof mkFbo>; blurB: ReturnType<typeof mkFbo> } => ({
      main: mkFbo(canvas.width, canvas.height),
      glow: mkFbo(canvas.width, canvas.height),
      blurA: mkFbo(canvas.width, canvas.height),
      blurB: mkFbo(canvas.width, canvas.height),
    });
    const fb0 = mkSet();
    const fb1 = mkSet();
    const fbPick = (n: number) => (n % 2 === 0 ? fb0 : fb1);

    // 单 pass 绘制工具：绑定 FBO/tex + program + 全屏四边形
    const drawPass = (progP: WebGLProgram, fb: WebGLFramebuffer | null, tex?: WebGLTexture) => {
      gl.useProgram(progP);
      gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
      if (tex) {
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, tex);
        gl.uniform1i(gl.getUniformLocation(progP, 'uTex'), 0);
      }
      gl.drawArrays(gl.TRIANGLES, 0, 6);
    };

    // 温度轮询句柄（每秒）：从 hw_temperature 读 GPU（NVIDIA/AMD）温度做熔断守门——
    // 超 90℃ 自动退出，保护硬件（对齐 FurMark 温度告警语义）。
    let tempTimer: ReturnType<typeof setInterval> | null = null;
    const MAX_GPU_TEMP = 90;
    let tripped = false;
    const gpuTemp = async () => {
      try {
        const r = await api.callTool('hw_temperature', {});
        // 局部变量不叫 t：t 归 useT() 的取文案函数（守卫测试按 t('key') 字面量查表）。
        const gpuTempC = parseGpuTemp(r.text);
        setStats((s) => ({ ...s, temp: gpuTempC != null ? `${gpuTempC}℃` : 'N/A' }));
        if (gpuTempC != null && gpuTempC >= MAX_GPU_TEMP && !tripped) {
          tripped = true;
          finish(true, t('toolbelt.donut.tripped', { temp: gpuTempC, max: MAX_GPU_TEMP }));
        }
      } catch {
        setStats((s) => ({ ...s, temp: 'N/A' }));
      }
    };
    gpuTemp();
    tempTimer = setInterval(gpuTemp, 1000);

    // 渲染循环：每轮 setTimeout 批量提交 4 帧（每帧 5 pass = 20 个全屏 pass 一次入队）。
    // 关键：①批量化抵消 setTimeout 的 4ms clamp（4 帧挤进同一轮）；②k%2 双缓冲
    // 让帧间 FBO 零依赖，GPU 可跨帧流水线；③flush 后立即排下一轮，队列永不空。
    // 这三点合起来让 GPU 的渲染队列持续堆积，占空比趋近满载（FurMark 同哲学）。
    const t0 = performance.now();
    let raf = 0;
    const frame = () => {
      if (doneRef.current) return;
      const f0 = frames.current;
      for (let k = 0; k < 4; k++) {
        frames.current += 1;
        const elapsed = (performance.now() - t0) / 1000;
        const S = fbPick(k).main;
        // ── pass1 主场景：双分形 + 甜甜圈 SDF + 皮毛 → main[k%2] ──
        // uIter 恒定 512（mandelbrot 上限），不做 fps 自适应——自适应是负反馈：
        // fps 低→迭代降→负载降→fps 升→迭代升→……在低负载处震荡，压不满。
        // FurMark 手法是负载只增不减，维持每像素恒定重计算。
        gl.useProgram(prog);
        gl.uniform2f(uRes, canvas.width, canvas.height);
        gl.uniform1f(uTime, elapsed);
        gl.uniform1f(uIter, 512.0);
        gl.bindFramebuffer(gl.FRAMEBUFFER, S.fb);
        gl.viewport(0, 0, canvas.width, canvas.height);
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        // ── pass2 高亮提取 → glow[k%2] ──
        drawPass(progBright, fbPick(k).glow.fb, S.tex);
        // ── pass3 水平模糊 → blurA[k%2]（uniform 先于绘制）──
        gl.useProgram(progBlurH);
        gl.uniform2f(gl.getUniformLocation(progBlurH, 'uDir'), 1.0, 0.0);
        gl.uniform2f(gl.getUniformLocation(progBlurH, 'uRes'), canvas.width, canvas.height);
        drawPass(progBlurH, fbPick(k).blurA.fb, fbPick(k).glow.tex);
        // ── pass4 垂直模糊 → blurB[k%2] ──
        gl.useProgram(progBlurV);
        gl.uniform2f(gl.getUniformLocation(progBlurV, 'uDir'), 0.0, 1.0);
        gl.uniform2f(gl.getUniformLocation(progBlurV, 'uRes'), canvas.width, canvas.height);
        drawPass(progBlurV, fbPick(k).blurB.fb, fbPick(k).blurA.tex);
        // ── pass5 合成：主 + 光晕 → 屏幕（每 4 帧只 present 1 帧）──
        // 其余 3 帧画到离屏 FBO：present 到屏幕会被浏览器 vsync 锁成 60Hz 一拍，
        // GPU 每次画完就空转等 vblank。FurMark 全程离屏 + 只向窗口回显低分辨率预览；
        // 这里采用「3/4 离屏、1/4 上屏」——画面仍流畅，但 4 帧只有 1 次 present 开销，
        // 解除 vsync 对 GPU 队列的节流。离屏用 glow FBO（pass3 之后已闲置，
        // main/blurA/blurB 都在 pass5 被读，写自己会触发读写冲突）。
        gl.useProgram(progComp);
        const presentToScreen = k % 4 === 3;
        gl.bindFramebuffer(gl.FRAMEBUFFER, presentToScreen ? null : fbPick(k).glow.fb);
        gl.activeTexture(gl.TEXTURE0);
        gl.bindTexture(gl.TEXTURE_2D, S.tex);
        gl.uniform1i(gl.getUniformLocation(progComp, 'uMain'), 0);
        gl.activeTexture(gl.TEXTURE1);
        gl.bindTexture(gl.TEXTURE_2D, fbPick(k).blurB.tex);
        gl.uniform1i(gl.getUniformLocation(progComp, 'uGlow'), 1);
        gl.drawArrays(gl.TRIANGLES, 0, 6);
      }
      frames.current = f0 + 4; // 记真实提交帧数
      gl.flush(); // 强制提交 4 帧的 20 个 pass，GPU 队列立刻堆积
      raf = window.setTimeout(frame, 0);
    };
    raf = window.setTimeout(frame, 0);

    // Esc 退出
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') finish(true);
    };
    window.addEventListener('keydown', onKey);

    // FPS + 时长统计：真实帧差 / 秒（FurMark 口径）
    const start = Date.now();
    let lastCount = 0;
    let lastAt = start;
    const statTimer = setInterval(() => {
      if (doneRef.current) return;
      const now = Date.now();
      const dt = (now - lastAt) / 1000;
      const fps = dt > 0 ? Math.round(frames.current - lastCount) / dt : 0;
      lastCount = frames.current;
      lastAt = now;
      fpsRef.current = fps;
      setStats((s) => ({ ...s, fps: Math.round(fps), secs: Math.round((now - start) / 1000) }));
    }, 1000);

    return () => {
      window.clearTimeout(raf);
      window.removeEventListener('resize', resize);
      window.removeEventListener('keydown', onKey);
      document.body.style.overflow = '';
      if (tempTimer) clearInterval(tempTimer);
      clearInterval(statTimer);
      gl.deleteProgram(prog);
      gl.deleteShader(vs);
      gl.deleteShader(fs);
      gl.deleteBuffer(vbo);
      // 多 pass 链资源：4 个后处理 program + 双缓冲 FBO（fb0/fb1 各 4 组）
      gl.deleteProgram(progBright);
      gl.deleteProgram(progBlurH);
      gl.deleteProgram(progBlurV);
      gl.deleteProgram(progComp);
      for (const s of [fb0, fb1]) {
        for (const fbo of [s.main, s.glow, s.blurA, s.blurB]) {
          gl.deleteFramebuffer(fbo.fb);
          gl.deleteTexture(fbo.tex);
        }
      }
      const ext = gl.getExtension('WEBGL_lose_context');
      ext?.loseContext();
    };
  }, [onDone, finish]);

  return (
    <div className="gpu-donut-overlay">
      <canvas ref={canvasRef} className="gpu-donut-canvas" />
      <div className="gpu-donut-hud">
        <div className="gpu-donut-row">
          <span className="gpu-donut-label">FPS</span>
          <span className="gpu-donut-val">{stats.fps}</span>
        </div>
        <div className="gpu-donut-row">
          <span className="gpu-donut-label">{t('toolbelt.donut.hudTime')}</span>
          <span className="gpu-donut-val">{stats.secs}s</span>
        </div>
        <div className="gpu-donut-row">
          <span className="gpu-donut-label">{t('toolbelt.donut.hudTemp')}</span>
          <span className="gpu-donut-val">{stats.temp}</span>
        </div>
      </div>
      <button className="gpu-donut-exit" onClick={() => finish(true)}>
        <X size={14} /> {t('toolbelt.donut.exit')}
      </button>
    </div>
  );
}

