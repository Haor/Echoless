export function settleBootGate(
  fontsReady: PromiseLike<unknown> | null | undefined,
  timeout: PromiseLike<unknown>,
  lift: () => void,
): Promise<void> {
  let lifted = false;
  const liftOnce = () => {
    if (lifted) return;
    lifted = true;
    lift();
  };
  const observe = (gate: PromiseLike<unknown>) =>
    Promise.resolve(gate).then(liftOnce, liftOnce);

  return Promise.race([
    observe(fontsReady ?? Promise.resolve()),
    observe(timeout),
  ]);
}
