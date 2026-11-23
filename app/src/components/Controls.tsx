import { useState } from "react";

// 选项少 → 一排按钮(不做下拉)。
export function SegButtons<T extends string>({
  value,
  options,
  onChange,
  disabled = false,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
  disabled?: boolean;
}) {
  return (
    <div
      className={`segg ${disabled ? "dim" : ""}`}
      aria-disabled={disabled || undefined}
    >
      {options.map((o) => (
        <button
          type="button"
          key={o.value}
          className={`b ${o.value === value ? "active" : ""}`}
          disabled={disabled}
          onClick={() => {
            if (!Object.is(o.value, value)) onChange(o.value);
          }}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function normalizeFieldCommit(params: {
  text: string;
  currentValue: unknown;
  numeric: boolean;
  min?: number;
  max?: number;
  integer?: boolean;
}): { value: unknown; text: string; changed: boolean } {
  const { text, currentValue, numeric, min, max, integer } = params;
  const s = text.trim();
  let nextVal: unknown;
  if (s === "") {
    nextVal = null;
  } else if (numeric) {
    let n = Number(s);
    if (!Number.isFinite(n)) {
      nextVal = null;
    } else {
      if (integer) n = Math.round(n);
      if (min != null) n = Math.max(min, n);
      if (max != null) n = Math.min(max, n);
      nextVal = n;
    }
  } else {
    nextVal = s;
  }
  const normalizedText = nextVal == null ? "" : String(nextVal);
  const currentComparable =
    currentValue == null || currentValue === "" ? null : currentValue;
  return {
    value: nextVal,
    text: normalizedText,
    changed: !Object.is(nextVal, currentComparable),
  };
}

// number / string / path 输入:本地编辑,blur 或 Enter 提交。空 = null(生成 TOML 时省略)。
export function Field({
  value,
  numeric,
  placeholder,
  onCommit,
  disabled = false,
  min,
  max,
  integer,
}: {
  value: unknown;
  numeric: boolean;
  placeholder: string;
  onCommit: (v: unknown) => void;
  disabled?: boolean;
  min?: number;
  max?: number;
  integer?: boolean;
}) {
  const valueText = value == null ? "" : String(value);
  const [draft, setDraft] = useState({ source: valueText, text: valueText });
  const txt = draft.source === valueText ? draft.text : valueText;
  const commit = () => {
    const res = normalizeFieldCommit({
      text: txt,
      currentValue: value,
      numeric,
      min,
      max,
      integer,
    });
    setDraft({ source: valueText, text: res.text });
    if (res.changed) onCommit(res.value);
  };
  return (
    <input
      className="afield"
      value={txt}
      disabled={disabled}
      placeholder={placeholder}
      aria-label={placeholder}
      inputMode={numeric ? (integer ? "numeric" : "decimal") : "text"}
      min={min}
      max={max}
      step={numeric && integer ? 1 : undefined}
      spellCheck={false}
      onChange={(e) => setDraft({ source: valueText, text: e.target.value })}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
      }}
    />
  );
}
