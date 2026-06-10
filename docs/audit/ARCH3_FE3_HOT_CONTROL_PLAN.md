# ARCH-3 / FE-3 Hot-Control Plan

Audit source: `docs/audit/CODE_AUDIT.md`

## Scope

Reduce unnecessary runtime restarts while preserving the CLI sidecar architecture chosen by the current product direction.

## Decision

Keep the realtime engine as a sidecar process. Add narrow stdin hot controls for parameters that are safe to apply inside the processing loop.

This pass implements `near_delay_ms` as the first non-volume hot pipeline control:

- It only changes the near/mic alignment delay buffer in the processing thread.
- It does not require rebuilding CPAL streams, reopening devices, changing sample rate, or rebuilding the processor chain.
- It directly supports the existing delay probe workflow, which currently writes `near_delay_ms` and then restarts the run.

## Non-Goals

- Do not hot-switch mic/reference/output devices in this pass. Device changes still require stream rebuilds.
- Do not hot-switch `sample_rate` or `frame_ms`; they define buffer sizes and stream configs.
- Do not hot-switch engine kind or arbitrary processor params; most require processor reconfiguration or chain rebuild.
- Do not move realtime into `echoless-core`.

## Edits

1. Backend runtime control:
   - Add `{ "cmd": "set_near_delay_ms", "near_delay_ms": 0..MAX_NEAR_DELAY_MS }`.
   - Resize/retune the existing near-delay buffer in the processing thread.
   - Emit `near_delay_changed` status JSON.
   - Expose the control in `SUPPORTED_RUNTIME_CONTROLS`.

2. Frontend:
   - Add `setNearDelayMs()` API helper.
   - Treat a pipeline patch containing only `near_delay_ms` as hot-applicable while running.
   - Keep all other pipeline patches on the existing validate + restart path.

3. Documentation/ledger:
   - Keep ARCH-3/FE-3 partially scoped to this hot-control subset unless all restart-causing config paths are resolved.

## Verification

- `cargo test -p echoless-cli realtime::control --locked`
- `cargo test -p echoless-cli realtime::stats --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `pnpm -C app build`
- `git diff --check`
- `graphify update echoless`
