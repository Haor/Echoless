import { useEffect, useState } from "react";
import type { ParamSpec, Processor } from "../types";
import type { PipelineCfg } from "../api";
import { useI18n, type Lang } from "../i18n";
import { Hint } from "../components/Hint";

interface Props {
  processors: Processor[];
  kind: string;
  pipeline: PipelineCfg;
  params: Record<string, unknown>;
  onPipeline: (patch: Partial<PipelineCfg>) => void;
  onParam: (key: string, val: unknown) => void;
}

// 参数说明(悬浮 label 时提示)。缺省键无提示。
const DESC: Record<string, { en: string; zh: string }> = {
  sample_rate: {
    en: "Pipeline sample rate (must divide by 100). Restarts runtime.",
    zh: "管线采样率(须能被 100 整除)。改动会重启运行时。",
  },
  frame_ms: { en: "Realtime frame size. Restarts runtime.", zh: "实时帧长。改动会重启运行时。" },
  reference_channels: {
    en: "Far-end reference channel mode (mono is the stable baseline).",
    zh: "远端参考声道模式(mono 为稳定基线)。",
  },
  ns_level: {
    en: "Only effective when NS is on; NS is off by default.",
    zh: "仅在降噪开启时有效;降噪默认关闭。",
  },
  agc: {
    en: "Off by default; avoids volume pumping (loud/quiet swings).",
    zh: "默认关闭,避免音量泵动(忽大忽小)。",
  },
  initial_delay_ms: {
    en: "Initial stream delay hint; runtime still estimates dynamically.",
    zh: "初始延迟提示;运行时仍会动态估计。",
  },
  tail_ms: {
    en: "Echo tail length. Auto ≈ AEC3 default (~52ms).",
    zh: "回声拖尾长度。自动时走 AEC3 默认(约 52ms)。",
  },
  delay_num_filters: {
    en: "Delay search window size. Auto ≈ 5 (~608ms).",
    zh: "延迟搜索窗大小。自动约为 5(约 608ms)。",
  },
  linear_stable_echo_path: {
    en: "Assume a more linear/stable echo path (pure loopback). Off by default.",
    zh: "假设 echo path 更线性稳定(偏纯 loopback)。默认关闭。",
  },
  model: { en: "GGUF model path (required).", zh: "GGUF 模型路径(必填)。" },
  library: { en: "LocalVQE dynamic library path (auto if empty).", zh: "LocalVQE 动态库路径(留空自动)。" },
  threads: { en: "CPU threads (auto if empty).", zh: "CPU 线程数(留空自动)。" },
  noise_gate: { en: "LocalVQE noise gate.", zh: "LocalVQE 噪声门。" },
  noise_gate_threshold_dbfs: { en: "Noise gate threshold (dBFS).", zh: "噪声门阈值(dBFS)。" },
  intensity_ratio: { en: "RTX AEC strength.", zh: "RTX AEC 强度。" },
  runtime_dir: { en: "NVIDIA AFX runtime dir (auto if empty).", zh: "NVIDIA AFX runtime 目录(留空自动)。" },
  model_path: { en: "RTX AEC model path (auto if empty).", zh: "RTX AEC 模型路径(留空自动)。" },
  on_runtime_error: { en: "On backend runtime error: silence or bypass.", zh: "运行时出错时:静音或直通。" },
  use_default_gpu: { en: "Use the default GPU.", zh: "使用默认 GPU。" },
  disable_cuda_graph: { en: "Disable CUDA graph.", zh: "关闭 CUDA graph。" },
};

function backendLabel(kind: string, proc?: Processor): string {
  if (kind === "nvidia_afx_aec") return "NVAFX";
  if (kind === "sonora_aec3") return "AEC3";
  return proc?.label ?? kind;
}

// number / string / path 输入:本地编辑,blur 或 Enter 提交。空 = null(auto,生成 TOML 时省略)。
function Field({
  value,
  numeric,
  placeholder,
  onCommit,
}: {
  value: unknown;
  numeric: boolean;
  placeholder: string;
  onCommit: (v: unknown) => void;
}) {
  const [txt, setTxt] = useState(value == null ? "" : String(value));
  useEffect(() => setTxt(value == null ? "" : String(value)), [value]);
  const commit = () => {
    const s = txt.trim();
    if (s === "") return onCommit(null);
    if (numeric) {
      const n = Number(s);
      return onCommit(Number.isFinite(n) ? n : null);
    }
    onCommit(s);
  };
  return (
    <input
      className="afield"
      value={txt}
      placeholder={placeholder}
      inputMode={numeric ? "decimal" : "text"}
      spellCheck={false}
      onChange={(e) => setTxt(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
      }}
    />
  );
}

// 选项少 → 一排按钮(不做下拉)。
function SegButtons<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segg">
      {options.map((o) => (
        <button
          key={o.value}
          className={`b ${o.value === value ? "active" : ""}`}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function AdvancedPage({
  processors,
  kind,
  pipeline,
  params,
  onPipeline,
  onParam,
}: Props) {
  const { t, lang, setLang } = useI18n();
  const proc = processors.find((p) => p.kind === kind);
  const desc = (k: string) => DESC[k]?.[lang];

  const backendParams = Object.entries(proc?.params ?? {}).filter(
    ([k]) => k !== "reference_channels" && k !== "ns",
  );

  const control = (key: string, spec: ParamSpec) => {
    const val = params[key];
    if (spec.type === "bool") {
      return (
        <SegButtons
          value={val ? "on" : "off"}
          options={[
            { value: "on", label: "ON" },
            { value: "off", label: "OFF" },
          ]}
          onChange={(v) => onParam(key, v === "on")}
        />
      );
    }
    if (spec.type === "select") {
      return (
        <SegButtons
          value={String(val ?? spec.default ?? "")}
          options={(spec.values ?? []).map((v) => ({ value: v, label: v }))}
          onChange={(v) => onParam(key, v)}
        />
      );
    }
    return (
      <Field
        value={val}
        numeric={spec.type === "number"}
        placeholder={spec.required ? "required" : t("auto")}
        onCommit={(v) => onParam(key, v)}
      />
    );
  };

  const arow = (key: string, label: string, spec: ParamSpec) => {
    const reqOk =
      !spec.requires ||
      Object.entries(spec.requires).every(([rk, rv]) => params[rk] === rv);
    const d = desc(key);
    return (
      <div className={`arow ${reqOk ? "" : "dim"}`} key={key}>
        <Hint text={d}>
          <span className="alabel">{label}</span>
        </Hint>
        <span className="aval">{control(key, spec)}</span>
      </div>
    );
  };

  return (
    <div className="page">
      <div className="kick">
        <span className="d">
          <i />
          <i />
          <i />
        </span>{" "}
        {t("advNote")}
      </div>
      <hr className="hair" />

      <div className="asec">// {t("secPipeline")}</div>
      <div className="acols">
        <div className="arow">
          <Hint text={desc("sample_rate")}>
            <span className="alabel">{t("sampleRate")}</span>
          </Hint>
          <span className="aval">
            <SegButtons
              value={String(pipeline.sample_rate)}
              options={[16000, 48000].map((n) => ({
                value: String(n),
                label: String(n),
              }))}
              onChange={(v) => onPipeline({ sample_rate: Number(v) })}
            />
          </span>
        </div>
        <div className="arow">
          <Hint text={desc("frame_ms")}>
            <span className="alabel">{t("frameMs")}</span>
          </Hint>
          <span className="aval">
            <SegButtons
              value={String(pipeline.frame_ms)}
              options={[10, 20].map((n) => ({
                value: String(n),
                label: `${n} MS`,
              }))}
              onChange={(v) => onPipeline({ frame_ms: Number(v) })}
            />
          </span>
        </div>
        <div className="arow">
          <Hint text={desc("reference_channels")}>
            <span className="alabel">{t("referenceChannels")}</span>
          </Hint>
          <span className="aval">
            <SegButtons
              value={pipeline.reference_channels}
              options={[
                { value: "mono", label: "MONO" },
                { value: "stereo", label: "STEREO" },
              ]}
              onChange={(v) =>
                onPipeline({ reference_channels: v as "mono" | "stereo" })
              }
            />
          </span>
        </div>
      </div>

      <div className="asec">// {backendLabel(kind, proc)}</div>
      <div className="acols">
        {backendParams.length === 0 && (
          <div className="pnote">no parameters</div>
        )}
        {backendParams.map(([key, spec]) => arow(key, key, spec))}
      </div>

      <div className="asec">// {t("secSession")}</div>
      <div className="acols">
        <div className="arow">
          <span className="alabel">{t("language")}</span>
          <span className="aval">
            <SegButtons<Lang>
              value={lang}
              options={[
                { value: "en", label: "EN" },
                { value: "zh", label: "中文" },
              ]}
              onChange={setLang}
            />
          </span>
        </div>
      </div>
    </div>
  );
}
