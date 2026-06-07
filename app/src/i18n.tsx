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
  advanced: { en: "Advanced", zh: "高级" },
  diagnostics: { en: "Diagnostics", zh: "诊断" },

  kicker: {
    en: "Acoustic Echo Cancellation · Local",
    zh: "声学回声消除 · 本地",
  },

  removingEcho: { en: "Removing Echo", zh: "正在消除回声" },
  echoStopped: { en: "Echo Stopped", zh: "已停止" },
  unstable: { en: "Unstable", zh: "不稳定" },
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
  diagNote: { en: "Diagnostics · record evidence", zh: "诊断 · 记录证据" },
  secRecord: { en: "Record", zh: "录制" },
  secHealth: { en: "Health", zh: "健康" },
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
