import { open } from "@tauri-apps/plugin-dialog";
import type {
  NvafxCheck,
  NvafxDoctor,
  Platform,
  Processor,
} from "../types";
import { openPath } from "../api";
import { useI18n } from "../i18n";

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
    kind: "sonora_aec3",
    name: "AEC3",
    tier: { en: "DEFAULT", zh: "默认" },
    echo: 8,
    voice: 6,
    cost: "CPU · light",
    sr: "48k / 16k",
    os: "Win · mac · Linux",
  },
  {
    kind: "localvqe",
    name: "LOCALVQE",
    tier: { en: "EXPERIMENTAL", zh: "试验" },
    echo: 5,
    voice: 5,
    cost: "CPU · neural",
    sr: "16k only",
    os: "Win · mac · Linux",
  },
  {
    kind: "nvidia_afx_aec",
    name: "NVAFX",
    tier: { en: "CLEANEST VOICE", zh: "人声最干净" },
    echo: 7,
    voice: 10,
    cost: "GPU · Tensor Core",
    sr: "16k / 48k",
    os: "Windows · RTX",
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

interface Props {
  processors: Processor[];
  platform: Platform;
  kind: string;
  params: Record<string, unknown>;
  doctor: NvafxDoctor | null;
  dev: boolean;
  onSelect: (kind: string) => void;
  onParam: (key: string, val: unknown) => void;
  onRecheck: (runtimeDir?: string) => void;
  onSetup: () => void;
}

export function EnginePage({
  processors,
  platform,
  kind,
  params,
  doctor,
  dev,
  onSelect,
  onParam,
  onRecheck,
  onSetup,
}: Props) {
  const { t, lang } = useI18n();

  const proc = (k: string) => processors.find((p) => p.kind === k);
  // 开发态(dev)临时解开 NVAFX 平台/doctor 门槛,用于走通前端流程。
  const supported = (k: string) =>
    dev || (proc(k)?.platforms.includes(platform) ?? true);
  // 就绪判定:AEC3 永远就绪;LocalVQE 需模型;NVAFX 需 doctor 通过(dev 跳过)。
  const ready = (k: string): boolean => {
    if (!supported(k)) return false;
    if (k === "localvqe") return Boolean(params.model);
    if (k === "nvidia_afx_aec") return dev || Boolean(doctor?.ok);
    return true;
  };

  async function pickModel() {
    try {
      const sel = await open({
        directory: false,
        filters: [{ name: "GGUF", extensions: ["gguf"] }],
      });
      if (typeof sel === "string") onParam("model", sel);
    } catch {
      /* cancelled */
    }
  }
  async function pickRuntime() {
    try {
      const sel = await open({ directory: true });
      if (typeof sel === "string") {
        onParam("runtime_dir", sel);
        onRecheck(sel);
      }
    } catch {
      /* cancelled */
    }
  }

  const card = (p: Profile) => {
    const sup = supported(p.kind);
    const active = kind === p.kind;
    const rdy = ready(p.kind);
    const status = !sup
      ? "UNAVAILABLE"
      : rdy
        ? active
          ? t("active")
          : t("rdyReady")
        : t("rdySetup");
    return (
      <div
        className={`ecard ${active ? "active" : ""} ${sup ? "" : "na"}`}
        onClick={() => sup && onSelect(p.kind)}
      >
        <div className="eh">
          <span className="en">{p.name}</span>
          <span className={`etag ${rdy ? "" : sup ? "warn" : "na"}`}>
            {active && <i className="dot" />} {status}
          </span>
        </div>
        <div className="etier">{p.tier[lang]}</div>
        <Meter label="ECHO" n={p.echo} />
        <Meter label="VOICE" n={p.voice} />
        <div className="espec">
          <span>{p.cost}</span>
          <span className="sep">·</span>
          <span>{p.sr}</span>
        </div>
        <div className="espec os">{p.os}</div>
      </div>
    );
  };

  const nv = doctor?.report;
  const nvSupported = supported("nvidia_afx_aec");
  const nvReady = dev || Boolean(doctor?.ok);
  const problems = (nv?.checks ?? []).filter(
    (c) => c.status === "missing" || c.status === "unsupported",
  ).length;

  const checkPill = (c: NvafxCheck) => (
    <div className="echk" key={c.name}>
      <span className={`cpill ${c.status}`}>{c.status}</span>
      <span className="cname">{c.name}</span>
      <span className="cdetail" title={c.detail}>
        {c.detail}
      </span>
    </div>
  );

  return (
    <div className="page">
      <div className="kick">
        <span className="d">
          <i />
          <i />
          <i />
        </span>{" "}
        {t("engNote")}
      </div>
      <hr className="hair" />

      <div className="ecards">
        {card(PROFILES[0])}
        {card(PROFILES[1])}
      </div>

      {/* NVAFX 全宽:规格牌 + doctor 就绪清单 + runtime 选择 + Broadcast 建议 */}
      <div
        className={`ecard wide ${kind === "nvidia_afx_aec" ? "active" : ""} ${
          nvSupported ? "" : "na"
        }`}
        onClick={() => nvSupported && onSelect("nvidia_afx_aec")}
      >
        <div className="eh">
          <span className="en">NVAFX <i className="sub">· RTX AEC</i></span>
          <span
            className={`etag ${nvReady ? "" : nvSupported ? "warn" : "na"}`}
          >
            {kind === "nvidia_afx_aec" && <i className="dot" />}{" "}
            {dev && !doctor?.ok
              ? kind === "nvidia_afx_aec"
                ? `${t("active")} · DEV`
                : `${t("rdyReady")} · DEV`
              : !nvSupported
                ? "WINDOWS · RTX ONLY"
                : doctor?.ok
                  ? kind === "nvidia_afx_aec"
                    ? t("active")
                    : t("rdyReady")
                  : `${problems} ${t("rdyIssues")}`}
          </span>
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
            <div className="epair">
              <span className="mk">»</span> {t("engPair")}
            </div>
          </div>
          <div className="ecol nvcol">
            {!nvSupported ? (
              <div className="cdetail na">{t("engWinOnly")}</div>
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
                <div className="echks">
                  {(nv?.checks ?? []).map(checkPill)}
                </div>
                <div className="drow nvrt">
                  <span className="dk">RUNTIME</span>
                  <span
                    className="dpick"
                    onClick={(e) => {
                      e.stopPropagation();
                      pickRuntime();
                    }}
                    title={(params.runtime_dir as string) || nv?.runtime_dir}
                  >
                    {(params.runtime_dir as string) ||
                      nv?.runtime_dir ||
                      t("auto")}
                  </span>
                  <button
                    className="dopen"
                    onClick={(e) => {
                      e.stopPropagation();
                      onRecheck((params.runtime_dir as string) || undefined);
                    }}
                  >
                    {t("engRecheck")} <span className="mk">↻</span>
                  </button>
                  {!doctor?.ok && (
                    <button
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

      {/* LocalVQE 选中时:模型路径(required)。 */}
      {kind === "localvqe" && (
        <div className="drow">
          <span className="dk">MODEL</span>
          <span className="dpick" onClick={pickModel} title={String(params.model ?? "")}>
            {(params.model as string) || t("engPickModel")}
          </span>
          {params.model ? (
            <button
              className="dopen"
              onClick={() => openPath(String(params.model))}
            >
              {t("openFolder")} <span className="mk">&raquo;</span>
            </button>
          ) : (
            <span className="cdetail warn">{t("engModelReq")}</span>
          )}
        </div>
      )}
    </div>
  );
}
