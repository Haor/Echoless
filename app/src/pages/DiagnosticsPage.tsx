import { useI18n } from "../i18n";

// 占位:诊断页(下一步接 --diagnostic-dir/seconds 录制 + 健康计数)。
export function DiagnosticsPage() {
  const { t } = useI18n();
  return (
    <div className="page">
      <div className="kick">
        <span className="d">
          <i />
          <i />
          <i />
        </span>{" "}
        {t("diagNote")}
      </div>
      <hr className="hair" />
      <div className="psec">// {t("secRecord")}</div>
      <div className="pnote">
        [ REC ] dir · seconds → session · mic.wav / ref.wav / out.wav /
        stats.csv
      </div>
      <div className="psec">// {t("secHealth")}</div>
      <div className="pnote">
        input_drops · ref/output_underruns · stale_drops · diverged
      </div>
      <div className="pscaffold">{t("comingSoon")}</div>
    </div>
  );
}
