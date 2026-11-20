#!/usr/bin/env python3
"""把对比测试的汇总画成 README 用的条形图。

用法：chart.py <summary.json> <输出目录> [--html-only]

每张图先写成一份不依赖外部资源的 HTML（`<输出目录>/html/`），再用无头的 Chromium 截成 PNG。一共三张图（启动、内存、
CPU 和硬盘），每张有中文、英文两种文字和浅色、深色两种配色，文件名是 `perf-<图>-<语言>-<配色>.png`。

画法：每个指标一个小图，横向条形，线性刻度，从 0 起；条的右边直接写数值；每个小图里从小到大排。LiteMiyu 的条用强调色，
其余的用同一种中性灰——颜色在这里只做一件事：指出哪一条是 LiteMiyu。数值都取 `summary.json` 里的中位数，这个脚本不改数。
"""

import html
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

APPS = ["litemiyu", "claude", "codex", "pi", "dsh", "opencode", "opencode2"]
NAMES = {
    "litemiyu": "LiteMiyu",
    "claude": "Claude Code",
    "codex": "codex",
    "pi": "pi",
    "dsh": "dsh (dsh-tui)",
    "opencode": "opencode 1.x",
    "opencode2": "opencode 2.x",
}

THEMES = {
    "light": {
        "ink": "#0b0b0b",
        "ink2": "#52514e",
        "muted": "#6f6d68",
        "track": "#efeeea",
        "bar": "#a8a69e",
        "accent": "#6250d6",
        "rule": "#d8d6cf",
    },
    "dark": {
        "ink": "#f0efec",
        "ink2": "#c3c2b7",
        "muted": "#9a9891",
        "track": "#26262a",
        "bar": "#6f6d68",
        "accent": "#9085e9",
        "rule": "#3a3a3f",
    },
}

MB = 1024 * 1024


def med(app, *path):
    """取一个中位数；硬盘那几项本来就是一个数。"""
    node = app
    for key in path:
        node = node[key]
    return node["median"] if isinstance(node, dict) else node


def ms(value):
    if value == 0:
        return "0 ms"
    if value < 10:
        return f"{value:.1f} ms"
    if value < 100:
        return f"{value:.1f} ms"
    return f"{value:,.0f} ms"


def mem(kb):
    value = kb / 1024
    return f"{value:.1f} MB" if value < 100 else f"{value:,.0f} MB"


def disk(nbytes):
    value = nbytes / MB
    if value < 1:
        return f"{nbytes / 1024:.0f} kB"
    return f"{value:.1f} MB" if value < 100 else f"{value:,.0f} MB"


def install_bytes(app):
    d = app["disk"]
    return d.get("install_with_runtime_bytes") or d["install_bytes"]


# 每张图：(文件名, {语言: (标题, 副标题)}, 小图的列数, [小图…])；小图：({语言: 标题}, 取数, 写数)。
CHARTS = [
    (
        "startup",
        {
            "zh": ("启动速度", "从开始执行到可以打字，20 次的中位数。越短越好。"),
            "en": ("Startup time", "From exec to a usable input box, median of 20 runs. Shorter is better."),
        },
        2,
        [
            ({"zh": "热缓存", "en": "Warm cache"}, lambda a: med(a, "startup_warm", "ready_ms"), ms),
            ({"zh": "冷缓存", "en": "Cold cache"}, lambda a: med(a, "startup_cold", "ready_ms"), ms),
        ],
    ),
    (
        "memory",
        {
            "zh": ("内存占用", "程序和它启动的所有进程合计，5 次的中位数。越少越好。"),
            "en": ("Memory", "The program and every process it starts, median of 5 runs. Less is better."),
        },
        2,
        [
            ({"zh": "空闲 · RSS", "en": "Idle · RSS"}, lambda a: med(a, "memory_idle", "rss_kb"), mem),
            ({"zh": "一轮对话后 · RSS", "en": "After one turn · RSS"}, lambda a: med(a, "memory_used", "rss_kb"), mem),
            ({"zh": "空闲 · PSS", "en": "Idle · PSS"}, lambda a: med(a, "memory_idle", "pss_kb"), mem),
            ({"zh": "一轮对话后 · PSS", "en": "After one turn · PSS"}, lambda a: med(a, "memory_used", "pss_kb"), mem),
            ({"zh": "空闲 · 匿名内存", "en": "Idle · anonymous"}, lambda a: med(a, "memory_idle", "anon_kb"), mem),
            (
                {"zh": "一轮对话后 · 匿名内存", "en": "After one turn · anonymous"},
                lambda a: med(a, "memory_used", "anon_kb"),
                mem,
            ),
        ],
    ),
    (
        "cpu-disk",
        {
            "zh": ("CPU 和硬盘", "CPU 是 3 次的中位数，硬盘是安装和使用一轮以后的大小。越少越好。"),
            "en": ("CPU and disk", "CPU is the median of 3 runs; disk is measured after install and one turn. Less is better."),
        },
        2,
        [
            (
                {"zh": "空闲时每分钟用的 CPU 时间", "en": "CPU time per idle minute"},
                lambda a: med(a, "cpu_idle", "cpu_ms_per_min"),
                ms,
            ),
            (
                {"zh": "流式输出 20 秒用的 CPU 时间", "en": "CPU time for 20 s of streaming"},
                lambda a: med(a, "cpu_stream", "cpu_ms"),
                ms,
            ),
            (
                {"zh": "安装占用（含需要的 Node.js）", "en": "Install size (with Node.js where needed)"},
                install_bytes,
                disk,
            ),
            (
                {"zh": "数据占用（初始化加一轮对话）", "en": "Data written (first run plus one turn)"},
                lambda a: a["disk"]["data_bytes"],
                disk,
            ),
        ],
    ),
]

WIDTH = 860  # CSS 像素；截图时放大两倍。
PAD = 20
GAP = 36
ROW = 22
HEAD = 64
PANEL_HEAD = 30
PANEL_GAP = 26
LABEL = 108
VALUE = 96


def panel(title, rows, theme, width):
    """一个小图。`rows` 是 [(名字, 值, 写出来的值, 是不是 LiteMiyu)]，已经排好。"""
    track = width - LABEL - VALUE
    top = max(value for _, value, _, _ in rows) or 1
    out = [f'<div class="panel" style="width:{width}px"><div class="ptitle">{html.escape(title)}</div>']
    for name, value, text, own in rows:
        # 条从 0 起、照比例；量到的数不是 0 的至少画 2 像素，看得见这一条在。
        length = 0 if value == 0 else max(2, round(track * value / top))
        color = theme["accent"] if own else theme["bar"]
        weight = "600" if own else "400"
        ink = theme["ink"] if own else theme["ink2"]
        out.append(
            f'<div class="row"><div class="name" style="font-weight:{weight};color:{ink}">{html.escape(name)}</div>'
            f'<div class="track" style="width:{track}px"><div class="bar" style="width:{length}px;background:{color}"></div>'
            f'<div class="value" style="font-weight:{weight};color:{ink}">{html.escape(text)}</div></div></div>'
        )
    out.append("</div>")
    return "".join(out)


def page(chart, apps, lang, theme_name):
    _, heads, columns, panels = chart
    theme = THEMES[theme_name]
    title, subtitle = heads[lang]
    panel_width = (WIDTH - 2 * PAD - (columns - 1) * GAP) // columns
    body = []
    for titles, get, fmt in panels:
        rows = sorted(
            ((NAMES[key], get(apps[key]), fmt(get(apps[key])), key == "litemiyu") for key in APPS),
            key=lambda row: row[1],
        )
        body.append(panel(titles[lang], rows, theme, panel_width))
    lines = -(-len(panels) // columns)
    height = PAD + HEAD + lines * (PANEL_HEAD + len(APPS) * ROW) + (lines - 1) * PANEL_GAP + PAD
    font = '"Noto Sans CJK SC", "Noto Sans", "Segoe UI", sans-serif'
    css = f"""
html,body{{margin:0;background:transparent}}
body{{width:{WIDTH}px;height:{height}px;font-family:{font};color:{theme['ink']};-webkit-font-smoothing:antialiased}}
.wrap{{padding:{PAD}px}}
h1{{margin:0;font-size:20px;line-height:28px;font-weight:600}}
.sub{{font-size:13px;line-height:20px;color:{theme['muted']};height:{HEAD - 28}px}}
.grid{{display:grid;grid-template-columns:repeat({columns},{panel_width}px);column-gap:{GAP}px;row-gap:{PANEL_GAP}px}}
.ptitle{{font-size:13px;line-height:20px;height:{PANEL_HEAD - 7}px;margin-bottom:6px;font-weight:600;
border-bottom:1px solid {theme['rule']}}}
.row{{display:flex;align-items:center;height:{ROW}px;font-size:12.5px}}
.name{{width:{LABEL}px;flex:none;white-space:nowrap}}
.track{{position:relative;display:flex;align-items:center;height:{ROW}px}}
.bar{{height:12px;border-radius:0 3px 3px 0;flex:none}}
.value{{margin-left:6px;white-space:nowrap;font-variant-numeric:tabular-nums}}
"""
    doc = (
        f'<!doctype html><html lang="{lang}"><head><meta charset="utf-8"><title>{html.escape(title)}</title>'
        f"<style>{css}</style></head><body><div class=\"wrap\"><h1>{html.escape(title)}</h1>"
        f'<div class="sub">{html.escape(subtitle)}</div><div class="grid">{"".join(body)}</div></div></body></html>'
    )
    return doc, height


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if len(args) != 2:
        sys.exit(__doc__)
    summary, out = Path(args[0]), Path(args[1])
    apps = json.loads(summary.read_text())["apps"]
    pages = out / "html"
    pages.mkdir(parents=True, exist_ok=True)
    browser = shutil.which("chromium") or shutil.which("google-chrome-stable")
    for chart in CHARTS:
        for lang in ("zh", "en"):
            for theme in THEMES:
                doc, height = page(chart, apps, lang, theme)
                name = f"perf-{chart[0]}-{lang}-{theme}"
                source = pages / f"{name}.html"
                source.write_text(doc)
                if "--html-only" in sys.argv or not browser:
                    continue
                # 每次用一个新的临时用户目录，不连调试口：不会碰到机器上别的浏览器。
                with tempfile.TemporaryDirectory(prefix="lm-chart-") as profile:
                    subprocess.run(
                        [
                            browser,
                            "--headless=new",
                            "--no-sandbox",
                            "--disable-gpu",
                            "--hide-scrollbars",
                            f"--user-data-dir={profile}",
                            "--force-device-scale-factor=2",
                            "--default-background-color=00000000",
                            f"--window-size={WIDTH},{height}",
                            f"--screenshot={out / (name + '.png')}",
                            source.resolve().as_uri(),
                        ],
                        check=True,
                        stdout=subprocess.DEVNULL,
                        stderr=subprocess.DEVNULL,
                    )
                print(name, height)


if __name__ == "__main__":
    main()
