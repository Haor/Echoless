import { memo } from "react";
import { useRuntimeHealth } from "../runtimeTelemetry";
import {
  DiagnosticsPage,
  type DiagnosticsPageProps,
} from "../pages/DiagnosticsPage";

type RuntimeDiagnosticsPageProps = Omit<DiagnosticsPageProps, "health">;

export const RuntimeDiagnosticsPage = memo(function RuntimeDiagnosticsPage(
  props: RuntimeDiagnosticsPageProps,
) {
  const health = useRuntimeHealth();
  return <DiagnosticsPage {...props} health={health} />;
});
