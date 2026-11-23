import { useCallback, useEffect, useRef } from "react";
import { animate, scrambleText, utils } from "animejs";
import { outputLevelToGain } from "../api";
import { useI18n } from "../i18n";
import { Hint } from "./Hint";

// 输出音量(0-100:0=静音 / 50=原声 0dB / 100=3x +9.54dB)。悬停滚轮调节,点按静音/恢复。
const VOL_MIN = 0;
const VOL_MAX = 100;
const SCROLL_THRESHOLD = 120;
const MUTE_MEM_KEY = "echoless.premuteVol.v1";

function dbLabel(volume: number): string {
  const g = outputLevelToGain(volume);
  if (g <= 0) return "mute";
  const db = 20 * Math.log10(g);
  return `${db >= 0 ? "+" : ""}${db.toFixed(1)} dB`;
}

export function VolumeWheel({
  volume,
  onChange,
  invertWheel = false,
}: {
  volume: number;
  onChange: (v: number) => void;
  // macOS 自然滚动方向相反,由平台侧传入反转以统一「向上 = 增大」。
  invertWheel?: boolean;
}) {
  const vRef = useRef(volume);
  vRef.current = volume;
  const acc = useRef(0);
  const hoverRef = useRef(false);
  const dbRef = useRef<HTMLSpanElement>(null);
  const shown = useRef(false);
  const clearTimer = useRef(0);
  const animTargetRef = useRef<{ textContent: string } | null>(null);

  const stopAnim = useCallback(() => {
    if (!animTargetRef.current) return;
    utils.remove(animTargetRef.current);
    animTargetRef.current = null;
  }, []);

  const showDb = useCallback(
    (animated: boolean) => {
      const el = dbRef.current;
      if (!el) return;
      window.clearTimeout(clearTimer.current);
      el.classList.add("on");
      const text = ` · ${dbLabel(vRef.current)}`;
      stopAnim();
      if (animated && !shown.current) {
        shown.current = true;
        const target = { textContent: el.textContent ?? "" };
        animTargetRef.current = target;
        animate(target, {
          textContent: scrambleText({
            text,
            from: "center",
            duration: 480,
            cursor: "░▒▓",
            ease: "inOut",
            override: false,
          }),
          onUpdate: () => {
            el.textContent = target.textContent;
          },
          onComplete: () => {
            el.textContent = text;
            if (animTargetRef.current === target) animTargetRef.current = null;
          },
        } as never);
      } else {
        el.textContent = text;
      }
    },
    [stopAnim],
  );

  // 收回走 CSS 宽度收拢 + 淡出,过渡结束后清空文本。
  const hideDb = useCallback(() => {
    const el = dbRef.current;
    if (!el) return;
    shown.current = false;
    stopAnim();
    el.classList.remove("on");
    window.clearTimeout(clearTimer.current);
    clearTimer.current = window.setTimeout(() => {
      el.textContent = "";
    }, 320);
  }, [stopAnim]);

  useEffect(() => {
    if (hoverRef.current) showDb(false);
  }, [volume, showDb]);

  useEffect(() => {
    return () => {
      stopAnim();
      window.clearTimeout(clearTimer.current);
    };
  }, [stopAnim]);

  const { t } = useI18n();

  const toggleMute = useCallback(() => {
    if (vRef.current > 0) {
      try {
        localStorage.setItem(MUTE_MEM_KEY, String(vRef.current));
      } catch {
        /* 记忆失败不阻塞静音 */
      }
      onChange(0);
    } else {
      let mem = 50;
      try {
        mem = Number(localStorage.getItem(MUTE_MEM_KEY)) || 50;
      } catch {
        /* 读取失败回落原声 */
      }
      onChange(Math.max(1, Math.min(VOL_MAX, Math.round(mem))));
    }
  }, [onChange]);

  return (
    <Hint text={t("volMuteHint")} pos="top">
      <span
        className="vol"
        role="button"
        tabIndex={0}
        aria-label={t("volMuteHint")}
        onClick={toggleMute}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            toggleMute();
          }
        }}
        onMouseEnter={() => {
          hoverRef.current = true;
          showDb(true);
        }}
        onMouseLeave={() => {
          hoverRef.current = false;
          hideDb();
        }}
        onWheel={(e) => {
          e.preventDefault();
          acc.current += invertWheel ? -e.deltaY : e.deltaY;
          if (Math.abs(acc.current) < SCROLL_THRESHOLD) return;
          const dir = acc.current < 0 ? 1 : -1;
          acc.current = 0;
          const next = Math.max(VOL_MIN, Math.min(VOL_MAX, vRef.current + dir));
          if (next !== vRef.current) onChange(next);
        }}
      >
        VOL {volume <= 0 ? "MUTE" : volume}
        <span ref={dbRef} className="voldb" />
      </span>
    </Hint>
  );
}
