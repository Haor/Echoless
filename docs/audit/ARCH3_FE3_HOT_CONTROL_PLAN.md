# ARCH-3 / FE-3 Hot-Control Plan

Audit source: `docs/audit/CODE_AUDIT.md`

## Scope

Reduce unnecessary runtime restarts while preserving the CLI sidecar architecture chosen by the current product direction.

## Decision

Keep the realtime engine as a sidecar process. Add narrow stdin hot controls for parameters that are safe to apply inside the processing loop.

This plan starts with narrow hot controls that do not require stream or processor-chain rebuilds.

Implemented proof points:

1. `near_delay_ms`:

- It only changes the near/mic alignment delay buffer in the processing thread.
- It does not require rebuilding CPAL streams, reopening devices, changing sample rate, or rebuilding the processor chain.
- It directly supports the existing delay probe workflow. The probe itself still pauses the run to own the devices, but the value can be applied live in ordinary runtime adjustment.

2. AEC3 `initial_delay_ms`:

- It forwards a stream-delay hint through the existing `EchoProcessor::set_stream_delay_ms` hook.
- It only applies to AEC3; LocalVQE and RTX AEC do not currently expose an internal delay hint.
- Clearing the field maps to `0ms` at runtime so a previous hint can be removed without restarting.

## Non-Goals

- Do not hot-switch mic/reference/output devices in this pass. Device changes still require stream rebuilds.
- Do not hot-switch `sample_rate` or `frame_ms`; they define buffer sizes and stream configs.
- Do not hot-switch engine kind or arbitrary processor params; most require processor reconfiguration or chain rebuild.
- Do not move realtime into `echoless-core`.

## Edits

1. Backend runtime control:
   - Add `{ "cmd": "set_near_delay_ms", "near_delay_ms": 0..MAX_NEAR_DELAY_MS }`.
   - Resize/retune the existing near-delay buffer in the processing thread.
   - Add `{ "cmd": "set_initial_delay_ms", "initial_delay_ms": 0..MAX_INITIAL_DELAY_MS }`.
   - Forward the initial-delay hint to processor nodes through `ProcessorChain::set_stream_delay_ms`.
   - Emit `near_delay_changed` status JSON.
   - Emit `initial_delay_changed` status JSON.
   - Expose the control in `SUPPORTED_RUNTIME_CONTROLS`.

2. Frontend:
   - Add `setNearDelayMs()` API helper.
   - Treat a pipeline patch containing only `near_delay_ms` as hot-applicable while running.
   - Add `setInitialDelayMs()` API helper.
   - Treat AEC3 `initial_delay_ms` as hot-applicable while running.
   - Keep all other pipeline patches on the existing validate + restart path.

3. Documentation/ledger:
   - Keep ARCH-3/FE-3 partially scoped to this hot-control subset unless all restart-causing config paths are resolved.

## Verification

- `cargo test -p echoless-cli realtime::control --locked`
- `cargo test -p echoless-cli realtime::stats --locked`
- `cargo test -p echoless-processors chain::tests --locked`
- `cargo test -p echoless-processors sonora_aec3 --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo test --workspace --locked`
- `pnpm -C app build`
- `git diff --check`
- `graphify update echoless`
