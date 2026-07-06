#!/usr/bin/env python3
"""Echoless app icon master (1024px) — H2「一滴橙」定稿版。

炭黑底板 + 直角三杠(纸灰=人声 / 橙=参考 / 暗灰=残响)+ 白噪点颗粒。
提案页在仓库外层:../Design/icon-ideas.html(H2)。

两个平台变体(构图相同,只差底板边距/圆角):
  --variant mac  圆角 + ~10% 透明边距(macOS Dock 规范) → 仅喂给 icon.icns
  --variant win  满幅直角、无边距(Windows/Linux 规范) → 喂给 ico/png/Square*

用法(手动 regen,产物提交进仓库):
  python3 gen-icon.py --variant win /tmp/win.png
  python3 gen-icon.py --variant mac /tmp/mac.png
  cd app && pnpm tauri icon /tmp/win.png            # 生成全套(方形)
  pnpm tauri icon /tmp/mac.png -o /tmp/mac-icons     # 仅取圆角 icns
  cp /tmp/mac-icons/icon.icns src-tauri/icons/icon.icns
"""
from PIL import Image, ImageDraw
import argparse
import random
import os

SS = 4          # supersample
SIZE = 1024
S = SIZE * SS

BG = (29, 29, 27, 255)        # --bg #1d1d1b
LINE = (53, 53, 47, 255)      # --line #35352f
ACC = (255, 114, 53, 255)     # --acc #ff7235
PAPER = (214, 213, 205, 255)  # --t-strong #d6d5cd
MUT = (138, 137, 127, 255)    # 残响暗灰 #8a897f

# 平台变体:底板边距 M / 圆角半径 R(1024 坐标系)。
VARIANTS = {
    "mac": (100, 224),  # 圆角 + 边距,Dock 会再叠自己的 squircle 遮罩
    "win": (0, 0),      # 满幅直角,与任务栏其它图标同尺寸
}

# 三杠布局按底板区域(region = SIZE - 2*M)的比例定义,两变体共用同一构图。
# 分数取自 mac 原始几何(M=100 时还原为 x=300 / w=325,435,215 / y=350,480,610 / h=64)。
BAR_FX = 200 / 824
BAR_FH = 64 / 824
BARS_FRAC = [
    (325 / 824, PAPER, 250 / 824),
    (435 / 824, ACC, 380 / 824),
    (215 / 824, MUT, 510 / 824),
]


def render(variant: str) -> Image.Image:
    margin, radius = VARIANTS[variant]
    region = SIZE - 2 * margin          # 底板边长(1024 坐标系)
    x0 = margin + BAR_FX * region       # 三杠左缘
    bar_h = BAR_FH * region

    # 底板(超采样后缩,保边缘锐利)。满幅变体不描边(边线会贴死画布边缘)。
    base = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(base)
    rect = [margin * SS, margin * SS, S - margin * SS, S - margin * SS]
    if margin > 0:
        d.rounded_rectangle(rect, radius=radius * SS, fill=BG,
                            outline=LINE, width=6 * SS)
    else:
        d.rectangle(rect, fill=BG)
    img = base.resize((SIZE, SIZE), Image.LANCZOS)

    # 白噪点:逐像素稀疏白点(固定种子,字节稳定 → 跨构建可复现),裁进底板。
    mask = Image.new("L", (SIZE, SIZE), 0)
    md = ImageDraw.Draw(mask)
    plate = [margin, margin, SIZE - margin, SIZE - margin]
    if margin > 0:
        md.rounded_rectangle(plate, radius=radius, fill=255)
    else:
        md.rectangle(plate, fill=255)
    rng = random.Random(7)
    noise = Image.new("L", (SIZE, SIZE), 0)
    np_ = noise.load()
    mp = mask.load()
    for y in range(SIZE):
        for x in range(SIZE):
            if mp[x, y]:
                # 对齐 SVG 版:alpha = clamp(0.9*n - 0.55) * 0.5
                a = 0.9 * rng.random() - 0.55
                if a > 0:
                    np_[x, y] = int(a * 0.5 * 255)
    white = Image.new("RGBA", (SIZE, SIZE), (255, 255, 255, 255))
    img.paste(white, (0, 0), noise)

    # 直角三杠(超采样后缩)。
    bars = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    db = ImageDraw.Draw(bars)
    for (fw, col, fy) in BARS_FRAC:
        y = margin + fy * region
        w = fw * region
        db.rectangle(
            [round(x0 * SS), round(y * SS),
             round((x0 + w) * SS), round((y + bar_h) * SS)],
            fill=col,
        )
    img.alpha_composite(bars.resize((SIZE, SIZE), Image.LANCZOS))
    return img


def main() -> None:
    ap = argparse.ArgumentParser(description="Echoless icon master generator")
    ap.add_argument("out", nargs="?", help="输出 PNG 路径")
    ap.add_argument("--variant", choices=list(VARIANTS), default="mac",
                    help="mac=圆角带边距(icns);win=满幅直角(ico/png/Square)")
    args = ap.parse_args()

    out = args.out or os.path.join(
        os.path.dirname(__file__), "..", ".generated-icons",
        f"icon-master-{args.variant}-1024.png")
    os.makedirs(os.path.dirname(out), exist_ok=True)
    render(args.variant).save(out)
    print(f"wrote {out} ({args.variant})")


if __name__ == "__main__":
    main()
