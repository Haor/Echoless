import {
  createContext,
  useContext,
  useState,
  type ReactNode,
} from "react";

export type Lang = "en" | "zh";

// 文案字典。技术标识(设备名/参数键/MIC·REF·OUT·dBFS/ON·OFF/采样率数字)保留原文。
const D: Record<string, { en: string; zh: string }> = {
  overview: { en: "Overview", zh: "总览" },
  engine: { en: "Engine", zh: "引擎" },
  advanced: { en: "Advanced", zh: "高级" },
  diagnostics: { en: "Diagnostics", zh: "诊断" },

  kicker: {
    en: "Acoustic Echo Cancellation · Local",
    zh: "声学回声消除 · 本地",
  },

  removingEcho: { en: "Removing Echo", zh: "正在消除回声" },
  echoStopped: { en: "Echo Stopped", zh: "已停止" },
  unstable: { en: "Unstable", zh: "不稳定" },
  noReference: { en: "No Reference", zh: "无参考信号" },
  latency: { en: "Latency", zh: "延迟" },
  ms: { en: "MS", zh: "毫秒" },
  stable: { en: "Stable", zh: "稳定" },
  checkSetup: { en: "Check Setup", zh: "检查设置" },

  input: { en: "Input", zh: "输入" },
  model: { en: "Model", zh: "模型" },
  output: { en: "Output", zh: "输出" },
  noise: { en: "Noise", zh: "降噪" },
  // 术语保留英文(近端/参考 译成中文反而怪)。
  micNearEnd: { en: "Microphone · Near-end", zh: "Microphone · Near-end" },
  reference: { en: "Reference", zh: "Reference" },
  noLoopback: { en: "No Loopback", zh: "No Loopback" },
  installCable: { en: "install virtual cable", zh: "安装虚拟声卡" },
  reduceNoise: { en: "Reduce background noise", zh: "抑制背景噪声" },

  signal: { en: "Signal", zh: "Signal" },
  sigFlow: {
    en: "Near-end Mic + Ref » Clean Output",
    zh: "Near-end Mic + Ref » Clean Output",
  },

  backToOverview: { en: "Overview", zh: "返回总览" },

  // Engine
  engNote: {
    en: "Pick the echo-removal brain · set it up here",
    zh: "选消回声引擎 · 在这里备妥",
  },
  active: { en: "ACTIVE", zh: "运行中" },
  rdyReady: { en: "READY", zh: "就绪" },
  rdySetup: { en: "SET UP", zh: "待配置" },
  rdyIssues: { en: "ISSUES", zh: "项待处理" },
  engPair: {
    en: "pair with NVIDIA Broadcast for residual noise",
    zh: "建议后接 NVIDIA Broadcast 消残留噪声",
  },
  engWinOnly: {
    en: "Windows + RTX GPU only · unavailable on this OS",
    zh: "仅 Windows + RTX 显卡 · 当前系统不可用",
  },
  engNoGpu: { en: "no NVIDIA GPU detected", zh: "未检测到 NVIDIA GPU" },
  engRecheck: { en: "recheck", zh: "重检" },
  engPickModel: { en: "pick .gguf model…", zh: "选择 .gguf 模型…" },
  engModelReq: { en: "model required", zh: "需要模型文件" },
  engSetupHint: { en: "set up in Engine", zh: "去 Engine 配置" },
  engSetupRtx: { en: "set up RTX", zh: "配置 RTX" },

  // RTX Setup 向导
  rtxSetup: { en: "RTX SETUP", zh: "RTX 配置" },
  back: { en: "back", zh: "返回" },
  wzSystem: { en: "System", zh: "系统" },
  wzReadiness: { en: "Readiness", zh: "就绪进度" },
  wzAction: { en: "Action", zh: "操作" },
  wzGpu: { en: "GPU", zh: "GPU" },
  wzDriver: { en: "Driver", zh: "驱动" },
  wzRuntime: { en: "Runtime", zh: "运行时" },
  recheck: { en: "recheck", zh: "重检" },
  // 状态标题 / 说明
  stUnsupportedPlatform: { en: "Unavailable on this OS", zh: "当前系统不可用" },
  stUnsupportedGpu: { en: "GPU not supported", zh: "显卡不受支持" },
  stMissingDriver: { en: "NVIDIA driver required", zh: "需要 NVIDIA 驱动" },
  stDriverTooOld: { en: "Driver too old", zh: "驱动版本过旧" },
  stMissingVc: { en: "VC++ runtime required", zh: "需要 VC++ 运行库" },
  stRuntimeMissing: { en: "Install RTX runtime", zh: "安装 RTX 运行时" },
  stModelMissing: { en: "Install RTX model", zh: "安装 RTX 模型" },
  stReady: { en: "RTX AEC ready", zh: "RTX AEC 就绪" },
  wzHardBlock: {
    en: "RTX AEC needs Windows + an RTX / Tensor-Core GPU (Turing / Ampere / Ada / Blackwell).",
    zh: "RTX AEC 需要 Windows + RTX / Tensor Core 显卡(Turing / Ampere / Ada / Blackwell)。",
  },
  wzOpenDriver: { en: "open NVIDIA drivers", zh: "打开 NVIDIA 驱动下载" },
  wzOpenVc: { en: "open VC++ redistributable", zh: "打开 VC++ 运行库下载" },
  wzInstallTitle: { en: "Install RTX AEC runtime", zh: "安装 RTX AEC 运行时" },
  wzInstallSize: {
    en: "runtime ~1 GB + model · extracted via Echoless CLI",
    zh: "运行时约 1 GB + 模型 · 由 Echoless CLI 解压",
  },
  wzSource: { en: "Source", zh: "来源" },
  wzLocalZip: { en: "Local zip", zh: "本地 zip" },
  wzDownload: { en: "Download", zh: "下载" },
  wzCommon: { en: "common runtime", zh: "公共运行时" },
  wzModel: { en: "model", zh: "模型" },
  wzPickZip: { en: "pick .zip…", zh: "选择 .zip…" },
  wzAutoArch: { en: "auto", zh: "自动" },
  wzArchMismatch: { en: "zip name does not match", zh: "zip 文件名与架构不符" },
  wzInstall: { en: "install", zh: "安装" },
  wzInstalling: { en: "installing… extracting, may take a minute", zh: "安装中… 解压中,可能需要一会" },
  wzDownloadSrc: {
    en: "from GitHub public release · auto-matches your GPU model",
    zh: "来自 GitHub 公共 release · 自动匹配你的 GPU 模型",
  },
  wzDownloadInstall: { en: "download & install", zh: "下载并安装" },
  wzDownloading: { en: "downloading… ~1 GB, may take a while", zh: "下载中… 约 1 GB,可能需要一会" },
  wzUseEngine: { en: "use this engine", zh: "使用该引擎" },
  wzNoGpuArch: { en: "fix GPU / driver detection first", zh: "请先修复 GPU / 驱动检测" },

  // Advanced
  advNote: {
    en: "Advanced parameters · validated before apply",
    zh: "高级参数 · 应用前校验",
  },
  secPipeline: { en: "Pipeline", zh: "管线" },
  secSession: { en: "Session", zh: "会话" },
  sampleRate: { en: "Sample Rate", zh: "采样率" },
  frameMs: { en: "Frame", zh: "帧长" },
  referenceChannels: { en: "Reference Channels", zh: "参考声道" },
  language: { en: "Language", zh: "语言" },
  auto: { en: "auto", zh: "自动" },
  applyHint: {
    en: "changes restart the runtime",
    zh: "改动会重启运行时",
  },
  needsRestart: { en: "needs restart", zh: "需重启" },

  // Diagnostics
  diagNote: {
    en: "Record a session to capture issues",
    zh: "录一段会话,留存问题现场",
  },
  openFolder: { en: "open", zh: "打开" },
  secRecord: { en: "Record", zh: "录制" },
  secHealth: { en: "Health", zh: "健康" },
  record: { en: "Record", zh: "录制" },
  maxSeconds: { en: "Max Seconds", zh: "最长秒数" },
  unlimited: { en: "unlimited", zh: "不限" },
  recordDir: { en: "Output Dir", zh: "输出目录" },
  choose: { en: "choose…", zh: "选择…" },
  recording: { en: "recording…", zh: "录制中…" },
  recordHint: {
    en: "writes mic / ref / out .wav + stats.csv",
    zh: "写出 mic / ref / out .wav + stats.csv",
  },
  notRunning: { en: "turn ON to record", zh: "开启后开始录制" },
  comingSoon: { en: "coming next", zh: "下一步填充" },
};

interface Ctx {
  lang: Lang;
  setLang: (l: Lang) => void;
  t: (k: keyof typeof D | string) => string;
}

const LangCtx = createContext<Ctx>({
  lang: "en",
  setLang: () => {},
  t: (k) => String(k),
});

export function LangProvider({ children }: { children: ReactNode }) {
  const [lang, setLangState] = useState<Lang>(() => {
    try {
      const v = localStorage.getItem("echoless.lang");
      return v === "zh" ? "zh" : "en";
    } catch {
      return "en";
    }
  });
  const setLang = (l: Lang) => {
    setLangState(l);
    try {
      localStorage.setItem("echoless.lang", l);
    } catch {
      /* ignore */
    }
  };
  const t = (k: string) => D[k]?.[lang] ?? k;
  return (
    <LangCtx.Provider value={{ lang, setLang, t }}>
      {children}
    </LangCtx.Provider>
  );
}

export const useI18n = () => useContext(LangCtx);
