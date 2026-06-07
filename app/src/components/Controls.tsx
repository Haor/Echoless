import { useEffect, useState } from "react";

// 选项少 → 一排按钮(不做下拉)。
export function SegButtons<T extends string>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (v: T) => void;
}) {
  return (
    <div className="segg">
      {options.map((o) => (
        <button
          key={o.value}
          className={`b ${o.value === value ? "active" : ""}`}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

// number / string / path 输入:本地编辑,blur 或 Enter 提交。空 = null(生成 TOML 时省略)。
export function Field({
  value,
  numeric,
  placeholder,
  onCommit,
  wide,
}: {
  value: unknown;
  numeric: boolean;
  placeholder: string;
  onCommit: (v: unknown) => void;
  wide?: boolean;
}) {
  const [txt, setTxt] = useState(value == null ? "" : String(value));
  useEffect(() => setTxt(value == null ? "" : String(value)), [value]);
  const commit = () => {
    const s = txt.trim();
    if (s === "") return onCommit(null);
    if (numeric) {
      const n = Number(s);
      return onCommit(Number.isFinite(n) ? n : null);
    }
    onCommit(s);
  };
  return (
    <input
      className={`afield ${wide ? "wide" : ""}`}
      value={txt}
      placeholder={placeholder}
      inputMode={numeric ? "decimal" : "text"}
      spellCheck={false}
      onChange={(e) => setTxt(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
      }}
    />
  );
}
