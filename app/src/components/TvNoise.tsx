import { useEffect, useRef } from "react";

// WebGL value-noise turbulence shader (CSS pixel lattice, premultiplied-safe opaque output).
const NOISE_FS = `precision highp float;
uniform float t;
uniform float px;
float h(vec2 p) {
  return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453123);
}
float vn(vec2 p, float s) {
  vec2 i = floor(p);
  vec2 f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(h(i + s), h(i + s + vec2(1.0, 0.0)), u.x),
    mix(h(i + s + vec2(0.0, 1.0)), h(i + s + vec2(1.0, 1.0)), u.x),
    u.y
  );
}
float tb(vec2 p, float s) {
  return abs(vn(p, s) * 2.0 - 1.0);
}
void main() {
  vec2 p = gl_FragCoord.xy * px;
  vec3 rgb = vec3(tb(p, t), tb(p, t + 17.0), tb(p, t + 41.0));
  float a = tb(p, t + 89.0);
  // Fold alpha into RGB for multiply blending so output stays opaque across WebView compositors.
  gl_FragColor = vec4(mix(vec3(1.0), rgb, a), 1.0);
}`;

const NOISE_VS = `attribute vec2 a;
void main() { gl_Position = vec4(a, 0.0, 1.0); }`;

export function TvNoise({ active }: { active: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef(active);
  activeRef.current = active;
  const scheduleRef = useRef<() => void>(() => {});

  useEffect(() => {
    const canvas = ref.current;
    const wrap = wrapRef.current;
    if (!canvas) return;
    const gl = canvas.getContext("webgl", {
      alpha: false,
      antialias: false,
      depth: false,
      stencil: false,
      powerPreference: "low-power",
    });
    if (!gl) return;

    let tLoc: WebGLUniformLocation | null = null;
    let pxLoc: WebGLUniformLocation | null = null;
    let ready = false;
    let forceFit = false;

    const initGL = (): boolean => {
      const compile = (type: number, src: string) => {
        const s = gl.createShader(type)!;
        gl.shaderSource(s, src);
        gl.compileShader(s);
        return s;
      };
      const prog = gl.createProgram()!;
      gl.attachShader(prog, compile(gl.VERTEX_SHADER, NOISE_VS));
      gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, NOISE_FS));
      gl.linkProgram(prog);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) return false;
      gl.useProgram(prog);
      const buf = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(
        gl.ARRAY_BUFFER,
        new Float32Array([-1, -1, 3, -1, -1, 3]),
        gl.STATIC_DRAW,
      );
      const loc = gl.getAttribLocation(prog, "a");
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
      tLoc = gl.getUniformLocation(prog, "t");
      pxLoc = gl.getUniformLocation(prog, "px");
      forceFit = true;
      return true;
    };
    ready = initGL();

    const fit = () => {
      const dpr = window.devicePixelRatio || 1;
      const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
      const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
      if (forceFit || canvas.width !== w || canvas.height !== h) {
        canvas.width = w;
        canvas.height = h;
        gl.viewport(0, 0, w, h);
        forceFit = false;
      }
      gl.uniform1f(pxLoc, 1.35 / dpr);
    };

    let raf = 0;
    let seed = 2;
    let focused = document.hasFocus();
    const shouldRun = () => !document.hidden && focused && activeRef.current;

    const draw = () => {
      if (!ready) return;
      gl.uniform1f(tLoc, seed);
      gl.drawArrays(gl.TRIANGLES, 0, 3);
    };
    const stop = () => {
      if (raf) cancelAnimationFrame(raf);
      raf = 0;
    };
    const frame = () => {
      raf = 0;
      seed = (seed + 0.618) % 61.0;
      draw();
      if (shouldRun()) raf = requestAnimationFrame(frame);
    };
    const schedule = () => {
      if (ready && !raf && shouldRun()) raf = requestAnimationFrame(frame);
    };
    scheduleRef.current = schedule;

    const onVisibility = () => {
      if (document.hidden) stop();
      else schedule();
    };
    const onFocus = () => {
      focused = true;
      schedule();
    };
    const onBlur = () => {
      focused = false;
      stop();
    };
    const onLost = (e: Event) => {
      e.preventDefault();
      ready = false;
      stop();
      if (wrap) wrap.classList.add("ctxlost");
    };
    const onRestored = () => {
      ready = initGL();
      if (wrap) wrap.classList.remove("ctxlost");
      if (ready) {
        fit();
        draw();
      }
      schedule();
    };

    if (ready) {
      fit();
      draw();
    }
    canvas.addEventListener("webglcontextlost", onLost, false);
    canvas.addEventListener("webglcontextrestored", onRestored, false);
    const ro = new ResizeObserver(() => {
      if (!ready) return;
      fit();
      draw();
    });
    ro.observe(canvas);
    document.addEventListener("visibilitychange", onVisibility);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    schedule();

    return () => {
      stop();
      ro.disconnect();
      document.removeEventListener("visibilitychange", onVisibility);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
      canvas.removeEventListener("webglcontextlost", onLost, false);
      canvas.removeEventListener("webglcontextrestored", onRestored, false);
      scheduleRef.current = () => {};
    };
  }, []);

  useEffect(() => {
    if (active) scheduleRef.current();
  }, [active]);

  return (
    <div className="tvnoise" aria-hidden="true" ref={wrapRef}>
      <canvas ref={ref} />
    </div>
  );
}
