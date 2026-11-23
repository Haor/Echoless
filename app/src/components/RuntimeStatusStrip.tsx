import { memo } from "react";
import { openUrl } from "../api";
import { useI18n } from "../i18n";
import {
  useRunStatusKind,
  useRuntimeLive,
  type RunStatusKind,
} from "../runtimeTelemetry";
import { ScrambleText } from "./ScrambleText";
import { dash } from "../numeric";

// 直达「屏幕与系统音频录制」隐私面板;未知 anchor 时 macOS 回退打开隐私根页。
export const SYS_AUDIO_PRIVACY_URL =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_AudioCapture";

export { useRunStatusKind, type RunStatusKind };

export const RAIL_TEXT: Record<RunStatusKind, string> = {
  live: "MONITOR LIVE",
  noref: "REF SILENT",
  warn: "MONITOR LIVE",
  bypass: "AEC BYPASS",
  stopped: "MONITOR HELD",
};

const BOX_CLASS: Record<RunStatusKind, string> = {
  live: "box",
  noref: "box idle",
  warn: "box warn",
  bypass: "box stopped", // 穿透 = 用户主动的「关」态,同停机灰阶
  stopped: "box stopped",
};

// 状态盒(zb 电源格内的灯行;v12 起格子即容器,盒子褪框)。
export const RuntimeStatusStrip = memo(function RuntimeStatusStrip({
  statusKind,
}: {
  statusKind: RunStatusKind;
}) {
  const { t } = useI18n();
  const statusText =
    statusKind === "stopped"
      ? t("echoStopped")
      : statusKind === "bypass"
        ? t("bypassLive")
        : statusKind === "warn"
          ? t("unstable")
          : statusKind === "noref"
            ? t("noReference")
            : t("removingEcho");
  return (
    <div className="status">
      <span className={BOX_CLASS[statusKind]}>
        <span
          className={`sq ${statusKind === "live" || statusKind === "warn" ? "dot" : ""} ${statusKind === "noref" ? "tri" : ""}`}
        />{" "}
        <ScrambleText text={statusText} />
      </span>
    </div>
  );
});

// 电源格注脚(zsub):PIPELINE 延迟 + 右侧状态字/引导动作(v7 层级调低)。
export const RuntimeSubline = memo(function RuntimeSubline({
  statusKind,
  activeReady,
  sysAudioDenied,
  sysAudioUndet,
  onEngineSetup,
  onProbeSystemAudio,
  onCheckSetup,
}: {
  statusKind: RunStatusKind;
  activeReady: boolean;
  sysAudioDenied: boolean;
  sysAudioUndet: boolean;
  onEngineSetup: () => void;
  onProbeSystemAudio: () => void;
  onCheckSetup: () => void;
}) {
  const live = useRuntimeLive();
  const { t } = useI18n();
  return (
    <div className="zsub">
      <span className="m">
        PIPELINE <b>{dash(live.lat, 0)}</b> {t("ms")}
      </span>
      <span className="fdot">·</span>
      {!activeReady ? (
        <button type="button" className="m act plainbtn" onClick={onEngineSetup}>
          {t("engSetupHint")} <span className="mk">&raquo;</span>
        </button>
      ) : sysAudioDenied ? (
        <button
          type="button"
          className="m act plainbtn"
          onClick={() => openUrl(SYS_AUDIO_PRIVACY_URL)}
        >
          {t("sysAudioGrant")} <span className="mk">&raquo;</span>
        </button>
      ) : sysAudioUndet ? (
        <button
          type="button"
          className="m act plainbtn"
          onClick={onProbeSystemAudio}
        >
          {t("sysAudioRequest")} <span className="mk">&raquo;</span>
        </button>
      ) : statusKind === "warn" ? (
        <button type="button" className="m act plainbtn" onClick={onCheckSetup}>
          <ScrambleText text={t("checkSetup")} />
        </button>
      ) : (
        <span className="m">
          <ScrambleText text={t("stable")} />
        </span>
      )}
    </div>
  );
});
