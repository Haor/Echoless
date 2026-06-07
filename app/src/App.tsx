import { useEffect, useRef, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  buildConfigToml,
  doctorAudio,
  getPlatform,
  listDevices,
  listProcessors,
  onRunEvent,
  onRunExit,
  startRun,
  stopRun,
  validateConfig,
  type PipelineCfg,
} from "./api";
import type {
  AudioDevice,
  DeviceList,
  DoctorAudio,
  Platform,
  Processor,
} from "./types";
import { useI18n } from "./i18n";
import {
  AppIcon,
  CapClose,
  CapMax,
  CapMin,
  IcoInput,
  IcoModel,
  IcoNoise,
  IcoOutput,
} from "./components/icons";
import { FooterBars, Scope, type Telemetry } from "./components/Scope";
import { Dropdown } from "./components/Dropdown";
import { ScrambleText } from "./components/ScrambleText";
import { AdvancedPage } from "./pages/AdvancedPage";
import { DiagnosticsPage } from "./pages/DiagnosticsPage";

const appWindow = getCurrentWindow();

// 设备选择值统一用 stable_id(跨重启稳定;mic/output 配置直接吃它)。
// 选默认输出:优先虚拟声卡(VB-CABLE / BlackHole),否则系统默认。
function pickDefaultOutput(outs: AudioDevice[]): string {
  const virt = outs.find((d) => /cable|blackhole|vb-?audio/i.test(d.name));
  if (virt) return virt.stable_id;
  return (
    outs.find((d) => d.is_default)?.stable_id ?? outs[0]?.stable_id ?? "default"
  );
}
function pickDefaultInput(ins: AudioDevice[]): string {
  return ins.find((d) => d.is_default)?.stable_id ?? ins[0]?.stable_id ?? "default";
}

const MODELS: { kind: string; label: string }[] = [
  { kind: "sonora_aec3", label: "AEC3" },
  { kind: "localvqe", label: "LOCALVQE" },
  { kind: "nvidia_afx_aec", label: "NVAFX" },
];

function modelName(kind: string): string {
  return MODELS.find((m) => m.kind === kind)?.label ?? kind.toUpperCase();
}

// 由 manifest 推导某 backend 的 chain 参数默认值(reference_channels 归到 pipeline)。
function defaultParams(proc: Processor | undefined): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  if (!proc) return out;
  for (const [k, spec] of Object.entries(proc.params)) {
    if (k === "reference_channels") continue;
    out[k] =
      spec.default !== undefined
        ? spec.default
        : spec.type === "bool"
          ? false
          : null;
  }
  return out;
}

interface Live {
  mic: number | null;
  ref: number | null;
  out: number | null;
  lat: number | null;
  healthy: boolean;
}

export default function App() {
  const [platform, setPlatform] = useState<Platform>("macos");
  const [devices, setDevices] = useState<DeviceList | null>(null);
  const [processors, setProcessors] = useState<Processor[]>([]);
  const [selInput, setSelInput] = useState("default");
  const [selOutput, setSelOutput] = useState("default");
  const [kind, setKind] = useState("sonora_aec3");
  const [pipeline, setPipeline] = useState<PipelineCfg>({
    sample_rate: 48000,
    frame_ms: 10,
    reference_channels: "mono",
  });
  const [params, setParams] = useState<Record<string, unknown>>({});
  const [running, setRunning] = useState(false); // 进程是否存活(含 restart 抖动)
  const [powerOn, setPowerOn] = useState(false); // 用户开关意图(UI 显示/动画只看这个)
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [view, setView] = useState<"overview" | "advanced" | "diagnostics">(
    "overview",
  );
  const [doctor, setDoctor] = useState<DoctorAudio | null>(null);
  // reference:可用源由 devices.reference_sources 提供;mac system 无 loopback → 默认退 none。
  const [reference, setReference] = useState("system");
  const [live, setLive] = useState<Live>({
    mic: null,
    ref: null,
    out: null,
    lat: null,
    healthy: true,
  });

  const telRef = useRef<Telemetry>({ mic: -120, ref: -120, out: -120, on: false });
  const runningRef = useRef(running);
  runningRef.current = running;
  const powerOnRef = useRef(powerOn);
  powerOnRef.current = powerOn;
  const pipelineRef = useRef(pipeline);
  pipelineRef.current = pipeline;
  const paramsRef = useRef(params);
  paramsRef.current = params;
  const { t } = useI18n();

  // 平台 + 设备/处理器枚举 + 事件订阅
  useEffect(() => {
    // 清理可能残留的 sidecar(前端 reload 后 Rust 子进程可能还活着 → 状态脱同步)。
    stopRun().catch(() => {});
    getPlatform().then(setPlatform).catch(() => {});
    refreshDevices();
    listProcessors()
      .then((m) => setProcessors(m.processors))
      .catch((e) => setErr(String(e)));
    doctorAudio().then(setDoctor).catch(() => {});

    const uns: UnlistenFn[] = [];
    (async () => {
      uns.push(
        await onRunEvent((ev) => {
          if (ev.type === "started") {
            telRef.current.on = true;
            setRunning(true);
            return;
          }
          // status
          const s = ev;
          const tel = telRef.current;
          tel.mic = s.mic_dbfs;
          tel.ref = s.ref_dbfs;
          tel.out = s.out_dbfs;
          tel.on = true;
          tel.micWave = s.mic_wave;
          tel.refWave = s.ref_wave;
          tel.outWave = s.out_wave;
          setLive({
            mic: s.mic_dbfs,
            ref: s.ref_dbfs,
            out: s.out_dbfs,
            lat: s.estimated_user_latency_ms,
            healthy:
              !s.diverged && s.runtime_errors === 0 && !s.last_backend_error,
          });
        }),
      );
      uns.push(
        await onRunExit(() => {
          telRef.current.on = false;
          setRunning(false);
        }),
      );
    })();
    return () => uns.forEach((u) => u());
  }, []);

  // Esc 始终有意义:在次级页按 Esc 返回 Overview。
  useEffect(() => {
    if (view === "overview") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setView("overview");
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [view]);

  // backend 切换 / manifest 加载 → 用该 backend 的 manifest 默认重置 chain 参数。
  useEffect(() => {
    setParams(defaultParams(processors.find((p) => p.kind === kind)));
  }, [processors, kind]);

  // 桌面 app:禁用 Tab 键焦点移动(避免按钮出现键盘选中框)。
  useEffect(() => {
    const onTab = (e: KeyboardEvent) => {
      if (e.key === "Tab") e.preventDefault();
    };
    window.addEventListener("keydown", onTab);
    return () => window.removeEventListener("keydown", onTab);
  }, []);

  function refreshDevices() {
    listDevices()
      .then((d) => {
        setDevices(d);
        setSelInput((cur) => (cur === "default" ? pickDefaultInput(d.inputs) : cur));
        setSelOutput((cur) =>
          cur === "default" ? pickDefaultOutput(d.outputs) : cur,
        );
        // 默认 reference:system 可用就用 system,否则退到 none;用户改过则保留。
        const sys = d.reference_sources.find((r) => r.id === "system");
        setReference((cur) =>
          cur !== "system" ? cur : sys && !sys.available ? "none" : "system",
        );
      })
      .catch((e) => setErr(String(e)));
  }

  type Override = Partial<{
    mic: string;
    output: string;
    reference: string;
    kind: string;
    pipeline: PipelineCfg;
    params: Record<string, unknown>;
  }>;

  function currentToml(over?: Override) {
    return buildConfigToml({
      mic: over?.mic ?? selInput,
      output: over?.output ?? selOutput,
      reference: over?.reference ?? reference,
      kind: over?.kind ?? kind,
      pipeline: over?.pipeline ?? pipelineRef.current,
      params: over?.params ?? paramsRef.current,
    });
  }

  async function start() {
    setBusy(true);
    setErr(null);
    try {
      const toml = currentToml();
      const v = await validateConfig(toml);
      if (!v.ok) {
        setErr(v.errors.map((e) => `${e.path}: ${e.message}`).join("; "));
        setBusy(false);
        return;
      }
      telRef.current.on = true;
      await startRun(toml, 80);
      setRunning(true);
      setPowerOn(true);
    } catch (e) {
      setErr(String(e));
      telRef.current.on = false;
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setBusy(true);
    try {
      await stopRun();
    } catch (e) {
      setErr(String(e));
    } finally {
      telRef.current.on = false;
      setRunning(false);
      setPowerOn(false);
      setBusy(false);
    }
  }

  async function togglePower() {
    if (busy) return;
    if (powerOn) await stop();
    else await start();
  }

  // 运行中改配置 → 重启 runtime 应用新值(后端契约要求)。
  // 成功路径不动 powerOn,避免状态框/开关跟着 scramble(各管各的)。
  async function applyChange(next: Override) {
    if (!powerOnRef.current) return;
    setBusy(true);
    try {
      await stopRun();
      const toml = currentToml(next);
      const v = await validateConfig(toml);
      if (!v.ok) {
        setErr(v.errors.map((e) => `${e.path}: ${e.message}`).join("; "));
        telRef.current.on = false;
        setRunning(false);
        setPowerOn(false);
        return;
      }
      telRef.current.on = true;
      await startRun(toml, 80);
      setRunning(true);
      setErr(null);
    } catch (e) {
      setErr(String(e));
      telRef.current.on = false;
      setRunning(false);
      setPowerOn(false);
    } finally {
      setBusy(false);
    }
  }

  // 切 backend:用新 backend 的 manifest 默认重置参数并重启。
  function changeKind(k: string) {
    const np = defaultParams(processors.find((p) => p.kind === k));
    setKind(k);
    setParams(np);
    applyChange({ kind: k, params: np });
  }
  // 改单个 chain 参数(NOISE / Advanced)。
  function setParam(key: string, val: unknown) {
    const np = { ...paramsRef.current, [key]: val };
    setParams(np);
    applyChange({ params: np });
  }
  // 改管线项(Advanced:sample_rate / frame_ms / reference_channels)。
  function changePipeline(patch: Partial<PipelineCfg>) {
    const npl = { ...pipelineRef.current, ...patch };
    setPipeline(npl);
    applyChange({ pipeline: npl });
  }

  const refOptions = (devices?.reference_sources ?? [])
    .filter((r) => r.available)
    .map((r) => ({
      value: r.selector ?? r.id,
      // input/output 同名设备(如 BlackHole 2ch)加方向标注以区分
      label:
        r.kind === "input"
          ? `${r.label} · in`
          : r.kind === "output"
            ? `${r.label} · out`
            : r.label,
    }));

  const isMac = platform === "macos";
  const off = !powerOn;
  const stopped = off;
  const unstable = powerOn && !live.healthy;
  const ns = Boolean(params.ns);
  // 降噪是 AEC3 管线独有(其它 backend 无 ns 参数)→ 不支持时置灰。
  const nsSupported = Boolean(
    processors.find((p) => p.kind === kind)?.params?.ns,
  );
  // 通话 app 里要选的"麦克风"名:由所选输出设备名推导(CABLE Input→CABLE Output;其余同名)。
  const outDev = devices?.outputs.find((d) => d.stable_id === selOutput);
  const cableName = outDev
    ? /cable input/i.test(outDev.name)
      ? outDev.name.replace(/input/i, "Output")
      : outDev.name
    : "CABLE Output";
  // footer 规格徽章随 pipeline 实时变。
  const stamp = `${pipeline.reference_channels.toUpperCase()} · ${
    pipeline.sample_rate / 1000
  }K · ${pipeline.frame_ms}MS`;

  const statusText = stopped
    ? t("echoStopped")
    : unstable
      ? t("unstable")
      : t("removingEcho");
  const boxClass = stopped ? "box stopped" : unstable ? "box warn" : "box";
  const viewTitle =
    view === "overview"
      ? t("overview")
      : view === "advanced"
        ? t("advanced")
        : t("diagnostics");

  const dash = (v: number | null, d = 1) =>
    v === null ? "—" : v.toFixed(d);

  return (
    <div className={`window ${isMac ? "mac" : "win"}`}>
      {/* ---- titlebar ---- */}
      <header className="tbar" data-tauri-drag-region>
        <AppIcon />
        <span className="screen">
          <ScrambleText text={viewTitle} />
        </span>
        <span className="hatch" />
        <span className="uid">{modelName(kind)}</span>
        {!isMac && (
          <span className="caption">
            <button className="cbtn" onClick={() => appWindow.minimize()}>
              <CapMin />
            </button>
            <button className="cbtn" onClick={() => appWindow.toggleMaximize()}>
              <CapMax />
            </button>
            <button className="cbtn close" onClick={() => appWindow.close()}>
              <CapClose />
            </button>
          </span>
        )}
      </header>

      {/* ---- content ---- */}
      <main className="content">
        {view === "overview" && (
        <>
        <div className="kick">
          <span className="d">
            <i />
            <i />
            <i />
          </span>{" "}
          {t("kicker")}
        </div>
        <div className="hero">
          <div className="word">ECHOLESS</div>
          {/* 物理滑动开关:主体方块在条纹轨道里左右滑动 + 标签 scramble */}
          <button
            className={`power ${off ? "off" : "on"}`}
            disabled={busy}
            onClick={togglePower}
          >
            <span className="slider">
              <ScrambleText text={off ? "OFF" : "ON"} trigger={powerOn} />
            </span>
          </button>
        </div>
        <div className="status">
          <span className={boxClass}>
            {/* 运行=圆点 ●,停止=方块 ■ */}
            <span className={`sq ${powerOn ? "dot" : ""}`} />{" "}
            <ScrambleText text={statusText} />
          </span>
          <span className="m">
            {t("latency")} <b>{dash(live.lat, 0)}</b> {t("ms")}
          </span>
          <span className="m">{unstable ? t("checkSetup") : t("stable")}</span>
        </div>

        <hr className="hair" />

        {/* ---- controls ---- */}
        <div className="rows">
          <div className="row">
            <span className="bul">•</span>
            <span className="k">{t("input")}</span>
            <span className="co">:</span>
            <span className="v">
              <Dropdown
                value={selInput}
                options={(devices?.inputs ?? []).map((d) => ({
                  value: d.stable_id,
                  label: d.name,
                }))}
                onChange={(v) => {
                  setSelInput(v);
                  applyChange({ mic: v });
                }}
              />
            </span>
            <span className="sp" />
            <span className="meta">{t("micNearEnd")}</span>
            <span className="ico">
              <IcoInput />
            </span>
          </div>

          <div className="row">
            <span className="bul">•</span>
            <span className="k">{t("model")}</span>
            <span className="co">:</span>
            <div className="segg" id="models">
              {MODELS.map((m) => {
                const proc = processors.find((p) => p.kind === m.kind);
                const supported =
                  !proc || proc.platforms.includes(platform);
                const exp = proc?.experimental;
                return (
                  <button
                    key={m.kind}
                    className={`b ${kind === m.kind ? "active" : ""} ${
                      exp ? "exp" : ""
                    }`}
                    disabled={!supported}
                    onClick={() => changeKind(m.kind)}
                  >
                    {m.label}
                  </button>
                );
              })}
            </div>
            <span className="sp" />
            <span className="meta">
              {t("reference")}{" "}
              <Dropdown
                compact
                align="right"
                warn={reference === "none"}
                value={reference}
                options={refOptions}
                onChange={(v) => {
                  setReference(v);
                  applyChange({ reference: v });
                }}
              />
            </span>
            <span className="ico">
              <IcoModel />
            </span>
          </div>

          <div className="row">
            <span className="bul">•</span>
            <span className="k">{t("output")}</span>
            <span className="co">:</span>
            <span className="v">
              <Dropdown
                value={selOutput}
                options={(devices?.outputs ?? []).map((d) => ({
                  value: d.stable_id,
                  label: d.name,
                }))}
                onChange={(v) => {
                  setSelOutput(v);
                  applyChange({ output: v });
                }}
              />
            </span>
            <span className="sp" />
            {doctor && !doctor.virtual_output_detected ? (
              <span className="meta" style={{ color: "var(--warn)" }}>
                <span className="mk">!!!</span> {t("installCable")}:{" "}
                <b>{doctor.recommended_driver}</b>
              </span>
            ) : (
              <span className="meta">
                <span className="mk">&gt;&gt;&gt;</span> in app pick{" "}
                <b>{cableName}</b> as mic
              </span>
            )}
            <span className="ico">
              <IcoOutput />
            </span>
          </div>

          <div className="row">
            <span className="bul">•</span>
            <span className="k">{t("noise")}</span>
            <span className="co">:</span>
            <div className={`segg ${nsSupported ? "" : "dim"}`} id="ns">
              <button
                className={`b ${ns ? "active" : ""}`}
                onClick={() => setParam("ns", true)}
              >
                ON
              </button>
              <button
                className={`b ${!ns ? "active" : ""}`}
                onClick={() => setParam("ns", false)}
              >
                OFF
              </button>
            </div>
            <span className="sp" />
            <span className="meta">
              {nsSupported ? t("reduceNoise") : "AEC3 only"}
            </span>
            <span className="ico">
              <IcoNoise />
            </span>
          </div>
        </div>

        <hr className="hair" />

        {/* ---- signal:三路示波 ---- */}
        <div className="sig">
          <div className="h">
            <span className="t">// {t("signal")}</span>
            <span className="v">{t("sigFlow")}</span>
          </div>
          <div className="scope">
            <div className="near">
              <div className="trace">
                <span className="lb">MIC</span>
                <Scope traceKey="mic" telRef={telRef} phase={0} />
                <span className="db">
                  {dash(live.mic)} <i>dBFS</i>
                </span>
              </div>
              <div className="trace">
                <span className="lb">REF</span>
                <Scope traceKey="ref" telRef={telRef} phase={2.1} />
                <span className="db">
                  {dash(live.ref)} <i>dBFS</i>
                </span>
              </div>
            </div>
            <div className="gap">&raquo;</div>
            <div className="far">
              <div className="trace">
                <span className="lb">OUT</span>
                <Scope traceKey="out" telRef={telRef} phase={4.2} />
                <span className="db">
                  {dash(live.out)} <i>dBFS</i>
                </span>
              </div>
            </div>
          </div>
        </div>
        </>
        )}
        {view === "advanced" && (
          <AdvancedPage
            processors={processors}
            kind={kind}
            pipeline={pipeline}
            params={params}
            onPipeline={changePipeline}
            onParam={setParam}
          />
        )}
        {view === "diagnostics" && <DiagnosticsPage />}
      </main>

      {/* ---- footer ---- */}
      <footer className="fbar">
        {view === "overview" ? (
          <>
            <button
              className="link"
              style={linkStyle}
              onClick={() => setView("advanced")}
            >
              {t("advanced")} <span className="mk">&gt;&gt;&gt;</span>
            </button>
            <button
              className="link"
              style={linkStyle}
              onClick={() => setView("diagnostics")}
            >
              {t("diagnostics")} <span className="mk">&gt;&gt;&gt;</span>
            </button>
          </>
        ) : (
          <button
            className="link"
            style={linkStyle}
            onClick={() => setView("overview")}
          >
            <span className="mk">&lt;&lt;&lt;</span> {t("backToOverview")}
          </button>
        )}
        <span className="sp" />
        {err ? (
          <span className="stamp" style={{ color: "var(--warn)" }} title={err}>
            {err.length > 48 ? err.slice(0, 48) + "…" : err}
          </span>
        ) : (
          <span className="stamp">{stamp}</span>
        )}
        <FooterBars telRef={telRef} />
      </footer>
    </div>
  );
}

const linkStyle: React.CSSProperties = {
  color: "var(--t-soft)",
  textDecoration: "none",
  display: "flex",
  alignItems: "center",
  gap: 7,
  background: "transparent",
  border: "none",
  font: "inherit",
  letterSpacing: "inherit",
  textTransform: "inherit",
};
