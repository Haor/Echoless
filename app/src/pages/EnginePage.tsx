import { useEffect, useState } from "react";
import type {
  NvafxCheck,
  NvafxDoctor,
  NoiseSuppressionManifest,
  Platform,
  Processor,
} from "../types";
import {
  macSystemInfo,
  openPath,
  type LocalvqeAssets,
  type MacSystemInfo,
} from "../api";
import { useI18n } from "../i18n";

// macOS 主版本号 → 代号(sw_vers 只给版本号,代号本地映射)。落不到映射时只显版本号。
function macOsName(version?: string | null): string | null {
  if (!version) return null;
  const major = version.split(".")[0];
  const NAMES: Record<string, string> = {
    "26": "Tahoe",
    "15": "Sequoia",
    "14": "Sonoma",
    "13": "Ventura",
    "12": "Monterey",
    "11": "Big Sur",
  };
  const name = NAMES[major];
  return name ? `macOS ${name} (${version})` : `macOS ${version}`;
}

// Official LocalVQE models from HF. The default is recommended, not bundled.
// 行内只标参数量(体积不上行 —— 列宽预算有限,详情看 hover title)。
const LVQE_MODELS: {
  file: string;
  ver: string;
  params: string;
  descriptionKey: string;
}[] = [
  {
    file: "localvqe-v1.4-aec-200K-f32.gguf",
    ver: "v1.4",
    params: "200K",
    descriptionKey: "lvqeModel14Description",
  },
  {
    file: "localvqe-v1.3-4.8M-f32.gguf",
    ver: "v1.3",
    params: "4.8M",
    descriptionKey: "lvqeModel13Description",
  },
  {
    file: "localvqe-v1.2-1.3M-f32.gguf",
    ver: "v1.2",
    params: "1.3M",
    descriptionKey: "lvqeModel12Description",
  },
];

// 引擎能力画像(前端描述性数据,非配置 contract)。
//   echo  = 消回声强度    voice = 人声干净度(neural 优势)
// NV 的差异化是「人声最干净」而非「消回声最强」;NV 模型有 16k/48k,Echoless 当前跑 48k。
interface Profile {
  kind: string;
  name: string;
  tier: { en: string; zh: string };
  echo: number; // 0..10
  voice: number; // 0..10
  cost: string;
  sr: string;
  os: string;
}
const PROFILES: Profile[] = [
  {
    kind: "aec3",
    name: "AEC3",
    tier: { en: "DEFAULT", zh: "默认" },
    echo: 9,
    voice: 6,
    cost: "CPU · webrtc",
    sr: "48k / 16k",
    os: "Win · mac · Linux",
  },
  {
    kind: "localvqe",
    name: "LOCALVQE",
    tier: { en: "EXPERIMENTAL", zh: "试验" },
    echo: 9,
    voice: 5,
    cost: "CPU · neural",
    sr: "48k (src) / 16k", // 模型原生 16k;48k 管线经 SRC 重采样进出
    os: "Win · mac · Linux",
  },
  {
    kind: "nvidia_afx_aec",
    name: "NVAFX",
    tier: { en: "ADVANCED", zh: "高级" },
    echo: 7,
    voice: 9,
    cost: "GPU · Tensor Core",
    sr: "48k",
    os: "Win only",
  },
];

function Meter({ label, n }: { label: string; n: number }) {
  return (
    <div className="emeter">
      <span className="el">{label}</span>
      <span className="ebar">
        {Array.from({ length: 10 }, (_, i) => (
          <i key={i} className={i < n ? "on" : ""} />
        ))}
      </span>
    </div>
  );
}

function checkPill(c: NvafxCheck) {
  return (
    <div className="echk" key={c.name}>
      <span className={`cpill ${c.status}`}>{c.status}</span>
      <span className="cname">{c.name}</span>
      <span className="cdetail" title={c.detail}>
        {c.detail}
      </span>
    </div>
  );
}

interface Props {
  processors: Processor[];
  noiseSuppression: NoiseSuppressionManifest | null;
  platform: Platform;
  kind: string;
  doctor: NvafxDoctor | null;
  dev: boolean;
  onSelect: (kind: string) => void;
  onPickModel: (path: string) => void;
  localvqeModel: string | null;
  lvAssets: LocalvqeAssets | null;
  lvDl: Record<string, number | null>;
  lvErr: string | null;
  onDownloadModel: (file: string) => void;
  onRefreshLvAssets: () => void;
  onRecheck: () => void;
  onSetup: () => void;
}

let macSysInfoCache: MacSystemInfo | null = null;
const DEV_MAC_SYS_INFO: MacSystemInfo = {
  model: "MacBook Pro",
  os_version: "26.5.1",
  chip: "Apple M4",
  memory_gb: 24,
  cores: 10,
};

function NvafxCard({
  kind,
  doctor,
  dev,
  platform,
  nvSupported,
  nvReady,
  problems,
  onSelect,
  onRecheck,
  onSetup,
}: {
  kind: string;
  doctor: NvafxDoctor | null;
  dev: boolean;
  platform: Platform;
  nvSupported: boolean;
  nvReady: boolean;
  problems: number;
  onSelect: (kind: string) => void;
  onRecheck: () => void;
  onSetup: () => void;
}) {
  const { t, lang } = useI18n();
  const nv = doctor?.report;
  const active = kind === "nvidia_afx_aec";

  // 不可用态(仅 macOS)拉本机系统信息填充右栏;dev 模拟使用临时样例,不污染真实缓存。
  const [realSysInfo, setRealSysInfo] = useState<MacSystemInfo | null>(
    macSysInfoCache,
  );
  useEffect(() => {
    if (nvSupported || platform !== "macos" || dev) return;
    if (realSysInfo) return;
    macSystemInfo()
      .then((info) => {
        macSysInfoCache = info;
        setRealSysInfo(info);
      })
      .catch(() => {});
  }, [nvSupported, platform, dev, realSysInfo]);
  const sysInfo = dev ? DEV_MAC_SYS_INFO : realSysInfo;

  return (
    <div
      className={`ecard wide ${active ? "active" : ""} ${nvSupported ? "" : "na"}`}
      role="button"
      tabIndex={nvSupported && !active ? 0 : -1}
      aria-pressed={active}
      aria-disabled={!nvSupported || active}
      onClick={() => nvSupported && !active && onSelect("nvidia_afx_aec")}
      onKeyDown={(e) => {
        if (
          nvSupported &&
          !active &&
          (e.key === "Enter" || e.key === " ")
        ) {
          e.preventDefault();
          onSelect("nvidia_afx_aec");
        }
      }}
    >
      <div className="eh">
        <span className="en">
          NVAFX <i className="sub">· RTX AEC</i>
        </span>
        <button
          type="button"
          className={`etag plainbtn ${nvReady ? "" : nvSupported ? "warn" : "na"}`}
          disabled={!nvSupported || active}
          aria-pressed={active}
          onClick={() => onSelect("nvidia_afx_aec")}
        >
          {active && <i className="dot" />}{" "}
          {dev && !doctor?.ok
            ? active
              ? `${t("active")} · DEV`
              : `${t("rdyReady")} · DEV`
            : !nvSupported
              ? t("windowsRtxOnly")
              : doctor?.ok
                ? active
                  ? t("active")
                  : t("rdyReady")
                : `${problems} ${t("rdyIssues")}`}
        </button>
      </div>
      <div className="etier">{PROFILES[2].tier[lang]}</div>
      <div className="ewrap">
        <div className="ecol">
          <Meter label="ECHO" n={PROFILES[2].echo} />
          <Meter label="VOICE" n={PROFILES[2].voice} />
          <div className="espec">
            <span>{PROFILES[2].cost}</span>
            <span className="sep">·</span>
            <span>{PROFILES[2].sr}</span>
          </div>
          <div className="espec os">{PROFILES[2].os}</div>
          {/* Maxine SDK 许可要求:集成应用须在应用内做品牌归属(README/release 已有,
              这里是 UI 侧唯一归属点)。 */}
          <div className="epair">powered by NVIDIA Maxine</div>
          <div className="epair">{t("engPair")}</div>
        </div>
        <div className={`ecol nvcol ${nvSupported ? "" : "nvna"}`}>
          {!nvSupported ? (
            <div className="nvnainfo">
              {sysInfo?.model && (
                <div className="nvnarow">
                  <span className="nvnak">{t("nvnaModel")}</span>
                  <span className="nvnav">{sysInfo.model}</span>
                </div>
              )}
              {macOsName(sysInfo?.os_version) && (
                <div className="nvnarow">
                  <span className="nvnak">{t("nvnaOs")}</span>
                  <span className="nvnav">{macOsName(sysInfo?.os_version)}</span>
                </div>
              )}
              {sysInfo?.chip && (
                <div className="nvnarow">
                  <span className="nvnak">{t("nvnaChip")}</span>
                  <span className="nvnav">
                    {sysInfo.chip}
                    {sysInfo.cores ? ` · ${sysInfo.cores}${t("nvnaCoresSuffix")}` : ""}
                  </span>
                </div>
              )}
              {sysInfo?.memory_gb != null && (
                <div className="nvnarow">
                  <span className="nvnak">{t("nvnaMemory")}</span>
                  <span className="nvnav">{sysInfo.memory_gb} GB</span>
                </div>
              )}
              <div className="cdetail na">{t("engWinOnly")}</div>
            </div>
          ) : (
            <>
              <div className="nvgpu">
                {nv && nv.gpus.length > 0 ? (
                  <>
                    {nv.gpus[0].name}
                    <i>
                      {" "}
                      · {nv.gpus[0].driver_version}
                      {nv.selected_arch ? ` · ${nv.selected_arch}` : ""}
                    </i>
                  </>
                ) : (
                  <span className="cdetail na">{t("engNoGpu")}</span>
                )}
              </div>
              <div className="echks">{(nv?.checks ?? []).map(checkPill)}</div>
              <div className="drow nvrt">
                <span className="dk">RUNTIME</span>
                <span className="dpath" title={nv?.runtime_dir}>
                  {nv?.runtime_dir || t("auto")}
                </span>
                <button
                  type="button"
                  className="dopen"
                  onClick={(e) => {
                    e.stopPropagation();
                    onRecheck();
                  }}
                >
                  {t("engRecheck")} <span className="mk">↻</span>
                </button>
                {!doctor?.ok && (
                  <button
                    type="button"
                    className="setupbtn"
                    onClick={(e) => {
                      e.stopPropagation();
                      onSetup();
                    }}
                  >
                    {t("engSetupRtx")} <span className="mk">&raquo;</span>
                  </button>
                )}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

export function EnginePage({
  processors,
  noiseSuppression,
  platform,
  kind,
  doctor,
  dev,
  onSelect,
  onPickModel,
  localvqeModel,
  lvAssets,
  lvDl,
  lvErr,
  onDownloadModel,
  onRefreshLvAssets,
  onRecheck,
  onSetup,
}: Props) {
  const { t, lang } = useI18n();

  useEffect(() => {
    onRefreshLvAssets();
  }, [onRefreshLvAssets]);

  const proc = (k: string) => processors.find((p) => p.kind === k);
  // 开发态(dev)临时解开 NVAFX 平台/doctor 门槛,用于走通前端流程。
  const supported = (k: string) =>
    dev || (proc(k)?.platforms.includes(platform) ?? true);
  // 就绪判定:AEC3 永远就绪;LocalVQE 需模型文件实际存在且 native_ready;NVAFX 需 doctor 通过(dev 跳过)。
  const ready = (k: string): boolean => {
    if (!supported(k)) return false;
    if (k === "localvqe") {
      return Boolean(
        localvqeModel &&
          lvAssets?.native_ready &&
          lvAssets.models.some((m) => m.path === localvqeModel),
      );
    }
    if (k === "nvidia_afx_aec") return dev || Boolean(doctor?.ok);
    return true;
  };

  // LocalVQE 模型清单(卡片内,checklist 盒子风格):绿=已下载可用,黄=未下载可点下载。
  // 点模型 = 选 LocalVQE 引擎 + 设该模型(onPickModel 原子处理),清单常驻不展开。
  const localvqeModels = () => (
    <div className="lvmods">
      {LVQE_MODELS.map((m) => {
        const capability = noiseSuppression?.localvqe_models.find(
          (entry) => entry.file === m.file,
        )?.capability;
        const capabilityLabel =
          capability === "pure_aec"
            ? "AEC"
            : capability === "built_in_ns"
              ? "NS"
              : null;
        const found = lvAssets?.models.find((x) => x.filename === m.file);
        const selected =
          kind === "localvqe" && !!found && localvqeModel === found.path;
        const downloading = m.file in lvDl;
        const pct = lvDl[m.file];
        const box = downloading
          ? pct != null
            ? `${pct}%`
            : "···"
          : selected
            ? "✓"
            : found
              ? "OK"
              : t("lvqeGet");
        return (
          <button
            type="button"
            key={m.file}
            className={`lvmod ${selected ? "on" : found ? "have" : "miss"}`}
            disabled={downloading || selected}
            onClick={(e) => {
              e.stopPropagation();
              if (selected) return;
              found ? onPickModel(found.path) : onDownloadModel(m.file);
            }}
            title={`${m.ver} · ${m.params} · ${t(m.descriptionKey)}`}
          >
            <span className={`lvbox ${found ? "ok" : "miss"}`}>{box}</span>
            <span className="lvver">{m.ver}</span>
            {capabilityLabel && <i className="lvdef">{capabilityLabel}</i>}
            <span className="lvsp" />
            <span className="lvms">
              <span className="lvp">{m.params}</span>
            </span>
          </button>
        );
      })}
      <div className="lvtools">
        <button
          type="button"
          className="dopen"
          onClick={(e) => {
            e.stopPropagation();
            if (lvAssets) openPath(lvAssets.models_dir);
          }}
          title={lvAssets?.models_dir}
        >
          {t("lvqeOpenDir")} <span className="mk">↗</span>
        </button>
      </div>
      {/* Native runtime is bundled with the app; warn only if missing in dev environments. */}
      {lvAssets && !lvAssets.native_ready && (
        <div className="cdetail warn" title={lvAssets.native_dir ?? undefined}>
          {t("lvqeRuntimeMissing")}
        </div>
      )}
      {lvErr && (
        <div className="cdetail warn lverr" title={lvErr}>
          {lvErr}
        </div>
      )}
    </div>
  );

  const card = (p: Profile) => {
    const sup = supported(p.kind);
    const active = kind === p.kind;
    const rdy = ready(p.kind);
    const isLvqe = p.kind === "localvqe";
    const status = !sup
      ? t("unavailable")
      : rdy
        ? active
          ? t("active")
          : t("rdyReady")
        : t("rdySetup");
    const meters = (
      <>
        <Meter label="ECHO" n={p.echo} />
        <Meter label="VOICE" n={p.voice} />
        <div className="espec">
          <span>{p.cost}</span>
          <span className="sep">·</span>
          <span>{p.sr}</span>
        </div>
        <div className="espec os">{p.os}</div>
      </>
    );
    const body = (
      <>
        <div className="eh">
          <span className="en">{p.name}</span>
          <span className={`etag ${rdy ? "" : sup ? "warn" : "na"}`}>
            {active && <i className="dot" />} {status}
          </span>
        </div>
        <div className="etier">{p.tier[lang]}</div>
        {isLvqe ? (
          <div className="ewrap">
            <div className="ecol">{meters}</div>
            <div className="ecol nvcol lvcol">{localvqeModels()}</div>
          </div>
        ) : (
          meters
        )}
      </>
    );
    if (isLvqe) {
      return (
        <div
          className={`ecard ${active ? "active" : ""} ${sup ? "" : "na"} lvwide`}
          role="button"
          tabIndex={sup && !active ? 0 : -1}
          aria-pressed={active}
          aria-disabled={!sup || active}
          onClick={() => sup && !active && onSelect(p.kind)}
          onKeyDown={(e) => {
            if (sup && !active && (e.key === "Enter" || e.key === " ")) {
              e.preventDefault();
              onSelect(p.kind);
            }
          }}
        >
          {body}
        </div>
      );
    }
    return (
      <button
        type="button"
        aria-pressed={active}
        disabled={!sup || active}
        className={`ecard cardbtn ${active ? "active" : ""} ${sup ? "" : "na"}`}
        onClick={() => onSelect(p.kind)}
      >
        {body}
      </button>
    );
  };

  const nv = doctor?.report;
  const nvSupported = supported("nvidia_afx_aec");
  const nvReady = dev || Boolean(doctor?.ok);
  const problems = (nv?.checks ?? []).filter(
    (c) => c.status === "missing" || c.status === "unsupported",
  ).length;

  return (
    <div className="page engine">
      <div className="kick">
        <span className="d">
          <i />
          <i />
          <i />
        </span>{" "}
        {t("engNote")}
      </div>
      <hr className="hair" />

      {/* 卡片区在 kick/分隔线之下的剩余空间里垂直居中(上下留白相等) */}
      <div className="enbody">
      <div className="ecards">
        {card(PROFILES[0])}
        {card(PROFILES[1])}
      </div>

      <NvafxCard
        kind={kind}
        doctor={doctor}
        dev={dev}
        platform={platform}
        nvSupported={nvSupported}
        nvReady={nvReady}
        problems={problems}
        onSelect={onSelect}
        onRecheck={onRecheck}
        onSetup={onSetup}
      />
      </div>

    </div>
  );
}
