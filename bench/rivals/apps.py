"""对比测试里的各家：怎么起、家目录里预先写什么、输入框长什么样、工具要确认时怎么放行。

每家只做这几件事：接到本机的假模型；跳过首次引导和目录信任提示；关掉自动更新和遥测。别的全是出厂默认。
目录都在 `root`（环境变量 RIVALS_ROOT）下面：`prefix/<家>` 是装好的程序，`home/<家>` 是家目录，`work/<家>` 是起程序时所在的目录。
"""

import json
import os
import re

MODEL = "fake-model"


def paths(root, app):
    return {"prefix": os.path.join(root, "prefix"), "home": os.path.join(root, "home", app),
            "work": os.path.join(root, "work", app), "tmp": os.path.join(root, "tmp", app)}


def base_env(root, app):
    p = paths(root, app)
    h = p["home"]
    return {
        "HOME": h, "XDG_CONFIG_HOME": f"{h}/.config", "XDG_DATA_HOME": f"{h}/.local/share",
        "XDG_STATE_HOME": f"{h}/.local/state", "XDG_CACHE_HOME": f"{h}/.cache",
        "PATH": f"{p['prefix']}/node/bin:/usr/bin:/bin", "TERM": "xterm-256color", "COLORTERM": "truecolor",
        "LANG": "en_US.UTF-8", "SHELL": "/bin/bash", "USER": os.environ.get("USER", "user"),
        "LOGNAME": os.environ.get("USER", "user"), "NO_PROXY": "127.0.0.1,localhost", "no_proxy": "127.0.0.1,localhost",
    }


def _write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


# ---- 输入框：交回框里现在的字（空框是 ""），框还没画出来是 None ----

def _after(lines, pattern, placeholder=None):
    """从下往上找第一行匹配 `pattern` 的（第一个分组是框里的字）；占位的提示字算空。"""
    rx = re.compile(pattern)
    for line in reversed(lines):
        m = rx.search(line)
        if m:
            text = m.group(1).strip().rstrip("│┃").strip()
            if placeholder and re.search(placeholder, text):
                return ""
            return text
    return None


class App:
    name = ""
    label = ""
    needs_node = False
    install_dirs = ()         # prefix 下面算这一家「本体」的目录
    approve_rule = None       # (屏幕上出现的正则, 要按的键, 说明)
    follow_rule = None        # 回答长过一屏以后不再跟着滚的家：(屏幕上出现的正则, 「回到底部」的键)
    first_run_keys = ()       # 初始化那一次要按的键：[(屏幕上出现的正则, 键)]

    def __init__(self, root, port):
        self.root, self.port = root, port
        self.p = paths(root, self.name)
        self.url = f"http://127.0.0.1:{port}"

    def env(self):
        return base_env(self.root, self.name)

    def argv(self):
        raise NotImplementedError

    def template(self):
        """往空的家目录里写配置（初始化之前）。"""

    def prepare(self, run):
        """初始化那一次之前还要做的事（`run(argv, env)` 在这一家的环境里跑一条命令）。"""

    def box(self, lines):
        raise NotImplementedError

    def runtime_dirs(self):
        return [os.path.join(self.p["prefix"], "node")] if self.needs_node else []

    def program_dirs(self):
        return [os.path.join(self.p["prefix"], d) for d in self.install_dirs]


class LiteMiyu(App):
    name, label = "litemiyu", "LiteMiyu"
    install_dirs = ("litemiyu",)

    def argv(self):
        return [f"{self.p['prefix']}/litemiyu/litemiyu"]

    def env(self):
        return {**super().env(), "FAKE_KEY": "fake"}

    def template(self):
        _write(f"{self.p['home']}/.config/litemiyu/config.toml", f"""[providers.fake]
driver = "openai-chat"
base_url = "{self.url}/v1"
key = {{ env = "FAKE_KEY" }}

[providers.fake.models.{MODEL}]
window = 200000

[models]
chat = "fake/{MODEL}"
""")

    def box(self, lines):
        return _after(lines, r"│ ?❯ ?(.*)$", r"^(/ opens the command list|Shift\+Enter starts a new line|.*…$)")

    # 回答长过一屏时视口停在开头让人从头读，出一个「Jump to bottom (Ctrl+End)」的按钮
    follow_rule = (r"Jump to bottom", b"\x1b[1;5F")


class Claude(App):
    name, label = "claude", "Claude Code"
    install_dirs = ("claude",)

    def argv(self):
        return [f"{self.p['prefix']}/claude/node_modules/.bin/claude"]

    def env(self):
        return {**super().env(), "CLAUDE_CONFIG_DIR": f"{self.p['home']}/.claude", "ANTHROPIC_BASE_URL": self.url,
                "ANTHROPIC_AUTH_TOKEN": "fake", "DISABLE_AUTOUPDATER": "1", "DISABLE_TELEMETRY": "1",
                "DISABLE_ERROR_REPORTING": "1", "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"}

    def template(self):
        d = f"{self.p['home']}/.claude"
        proj = {self.p["work"]: {"hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}}
        _write(f"{d}/.claude.json", json.dumps({"hasCompletedOnboarding": True, "theme": "dark", "projects": proj}))

    def box(self, lines):
        return _after(lines, r"^\s*[❯>] ?(.*)$", r"^Try \"")

    approve_rule = (r"Do you want to proceed\?", b"\r", "确认框里默认选中的是 Yes，按回车")


class Codex(App):
    name, label = "codex", "codex"
    install_dirs = ("codex",)
    needs_node = True

    def argv(self):
        return [f"{self.p['prefix']}/codex/node_modules/.bin/codex"]

    def env(self):
        return {**super().env(), "CODEX_HOME": f"{self.p['home']}/.codex", "FAKE_KEY": "fake"}

    def template(self):
        _write(f"{self.p['home']}/.codex/config.toml", f"""model_provider = "fake"
check_for_update_on_startup = false

[model_providers.fake]
name = "fake"
base_url = "{self.url}/v1"
env_key = "FAKE_KEY"
wire_api = "responses"

[analytics]
enabled = false

[projects."{self.p['work']}"]
trust_level = "trusted"
""")

    def box(self, lines):
        # 首次运行的选择菜单也用「›」标当前项（「› 1. Try new model」）：那不是输入框
        if any(re.search(r"enter/esc confirm|Press enter to continue|^\s*›? ?\d\. ", l) for l in lines):
            return None
        return _after(lines, r"^\s*› ?(.*)$", r"^(Ask Codex|Explain|Summarize|Find and fix|Write tests|Improve|Use /|Run /|Implement)")

    # 第一次运行会问要不要换成新出的模型：选「2. Use existing model」，照配置里写的模型用
    first_run_keys = ((r"2\. Use existing model", b"\x1b[B\r"),)

    approve_rule = (r"Would you like to run the following command\?", b"\r", "确认框里默认选中的是 Yes，按回车")


class Pi(App):
    name, label = "pi", "pi"
    install_dirs = ("pi",)
    needs_node = True

    def argv(self):
        return [f"{self.p['prefix']}/pi/node_modules/.bin/pi"]

    def env(self):
        return {**super().env(), "PI_CODING_AGENT_DIR": f"{self.p['home']}/.pi/agent", "PI_SKIP_VERSION_CHECK": "1",
                "PI_TELEMETRY": "0", "FAKE_KEY": "fake"}

    def template(self):
        d = f"{self.p['home']}/.pi/agent"
        _write(f"{d}/models.json", json.dumps({"providers": {"fake": {
            "name": "fake", "baseUrl": f"{self.url}/v1", "api": "openai-completions", "apiKey": "$FAKE_KEY",
            "models": [{"id": MODEL, "name": MODEL, "contextWindow": 200000, "maxTokens": 32000, "input": ["text"]}]}}}))
        _write(f"{d}/settings.json", json.dumps({"defaultProvider": "fake", "defaultModel": MODEL}))

    def box(self, lines):
        # 输入框是两条横线夹着的几行，没有提示符：取最下面一对横线之间的字
        rules = [i for i, l in enumerate(lines) if re.match(r"^─{20,}", l)]
        if len(rules) < 2:
            return None
        return " ".join(l.strip() for l in lines[rules[-2] + 1:rules[-1]]).strip()


class Dsh(App):
    name, label = "dsh", "dsh（dsh-tui）"
    install_dirs = ("dsh", "dsh-tui", "pnpm")
    needs_node = True

    def argv(self):
        return [f"{self.p['prefix']}/dsh-tui/node_modules/.bin/dsh-tui"]

    def env(self):
        e = super().env()
        pre = self.p["prefix"]
        e["PATH"] = f"{pre}/dsh/node_modules/.bin:{pre}/dsh-tui/node_modules/.bin:{pre}/pnpm/node_modules/.bin:" + e["PATH"]
        return {**e, "DSH_HOME": f"{self.p['home']}/.dsh", "FAKE_KEY": "fake"}

    def patch(self):
        prov = {"id": "llm-pi-ai", "config": {"providers": {"fake": {
            "displayName": "fake", "api": "openai-completions", "baseURL": f"{self.url}/v1", "apiKeyEnv": "FAKE_KEY",
            "defaultContextWindow": 200000, "defaultMaxTokens": 32000,
            "models": [{"id": MODEL, "name": MODEL, "contextWindow": 200000, "maxTokens": 32000, "input": ["text"]}]}}}}
        model = {"id": "agent-default-model", "config": {"provider": "fake", "model": MODEL}}
        return f"- {json.dumps(prov)}\n- {json.dumps(model)}\n"

    def template(self):
        _write(f"{self.p['home']}/.dsh-tui/onboarding.json", json.dumps({"completed": True, "version": 1}))
        _write(f"{self.p['home']}/.dsh/cordis.patch.yml", self.patch())

    def box(self, lines):
        # 首页的输入框是「│ ❯ … │」，对话里的是「⌸ ❯ … ⛶」；上面用户说过的话也以「❯」开头，但两边没有框线
        return _after(lines, r"[│┃⌸]\s*[❯>›] ?(.*?)\s*[│┃⛶]\s*$")


class Opencode(App):
    name, label = "opencode", "opencode 1.x"
    install_dirs = ("opencode",)

    def argv(self):
        return [f"{self.p['prefix']}/opencode/node_modules/.bin/opencode"]

    def template(self):
        cfg = {"$schema": "https://opencode.ai/config.json", "model": f"fake/{MODEL}", "small_model": f"fake/{MODEL}",
               "autoupdate": False,
               "provider": {"fake": {"npm": "@ai-sdk/openai-compatible", "name": "fake",
                                     "options": {"baseURL": f"{self.url}/v1", "apiKey": "fake"},
                                     "models": {MODEL: {"name": MODEL, "limit": {"context": 200000, "output": 32000}}}}}}
        _write(f"{self.p['home']}/.config/opencode/opencode.json", json.dumps(cfg))

    def box(self, lines):
        # 输入框是左边一条「┃」的几行：空行、打的字（可以折成几行）、空行、最后一行是「Build · 模型名」
        rows = [m.group(1).strip() for m in (re.match(r"^\s*┃(.*)$", l) for l in lines) if m]
        rows = [r for r in rows if r]
        if not rows or not rows[-1].startswith(("Build", "Plan")):
            return None
        text = " ".join(rows[:-1])
        return "" if text.startswith("Ask anything") else text


class Opencode2(Opencode):
    name, label = "opencode2", "opencode 2.x"
    install_dirs = ("opencode2",)

    def argv(self):
        return [f"{self.p['prefix']}/opencode2/opencode"]


class Baseline(App):
    """不是 harness：一个只把终端切到 raw、把打的字照原样回出来的最小程序（sh 起 stty 再换成 cat）。
    用来看量具自己的底：起一个进程、过一遍伪终端最少要多久。只量启动。"""
    name, label = "baseline", "参照：sh + stty + cat"
    install_dirs = ()

    def argv(self):
        return ["/usr/bin/sh", "-c", "stty raw -echo; printf '\\r\\033[K> '; exec cat"]

    def box(self, lines):
        return _after(lines, r"^> ?(.*)$")


APPS = {c.name: c for c in (LiteMiyu, Claude, Codex, Pi, Dsh, Opencode, Opencode2, Baseline)}
RIVALS = [n for n in APPS if n != "baseline"]


def get(name, root, port):
    return APPS[name](root, port)
