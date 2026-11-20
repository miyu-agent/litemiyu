#!/usr/bin/env python3
"""造一份假的模型目录（LM-9补）：和 models.dev 的 `api.json` 同样形状、同样规模，名字、数值照固定的种子生成，每次一字不差。

为什么要它：LM-9 起程序不带目录，第一次要用到时取来存进缓存（`<缓存>/models/models-dev.json`）。用户平常的状态是缓存里有目录：
发话时要读目录（第一次读整份、记下查目录的答，以后读记下的小表）、查到窗口和价格、底栏写花费。排函数（`order.py`）、量尺
（`measure.py`）的沙箱里没有目录的话，这几条路一条都录不到、量不到（LM-9 量到：有目录时 HTTPS 发过消息多 0.7 MB 代码页）。
仓库里不放真目录（6.8 MB，外来数据），放这个生成器。

形状照 `lm-kernel` 读目录认的格（`crates/lm-kernel/src/models/catalog.rs` 的 `RawProvider`、`RawModel`，`catalog/price.rs` 的
`RawCost`）全都写上，读的一方不认的格（`description`、`attachment`、`knowledge`、`canonical_model_id`……）也照真目录的样子写，
解析时要跳过它们，大小也才对得上：

- 供应商：`id`、`env`、`npm`、`api`（九成多有）、`name`、`doc`、`models`；
- 模型：`id`、`name`、`description`、`family`、`attachment`、`reasoning`、`reasoning_options`（`toggle`、`effort` 带几档、
  `budget_tokens`）、`tool_call`、`interleaved`（`{"field": …}` 或 `true`）、`structured_output`、`temperature`、`knowledge`、
  `release_date`、`last_updated`、`modalities`（`input`、`output`）、`open_weights`、`limit`（`context`、`output`，两成带 `input`）、
  `status`、`cost`（`input`、`output`、`cache_read`、`cache_write`、`reasoning`、`input_audio`，一成带 `tiers`、几十分之一带
  `context_over_200k`）、`canonical_model_id`、`provider`（`npm`、`api`、`shape`）、`type`、`experimental`。

规模、比例照 2026-11-19 取的那一份真目录量的：324 家、10000 个模型（真的 10005），一家的模型数
中位 10、最多 600 来个；模型名四千来种（聚合的几家写成 `<厂>/<名字>`，同一个名字好几十家都列）；每格出现的比例、价格的写法
几千种、思考强度一百多种。整份不缩进（真目录也不缩进），约 6.8 MB。

量尺、排函数用的那一家写死在里面（[`BENCH_PROVIDER`]、[`BENCH_MODEL`]）：编号、模型名和真目录里的一样（`deepseek`、
`deepseek-flash`），这样同一份配置拿真目录对照也认得出；窗口、最大输出、价格、思考强度的写法照真目录那一条的样子手写
（窗口 1048576、价格 0.15 / 0.6），底栏的花费算得出来。别的名字都是造的。

用法：catalog.py <输出文件>（没给的写到标准输出）；`python3 -c 'import catalog; catalog.text()'` 拿字。
"""

import json
import math
import random
import sys

SEED = 20261119
PROVIDERS = 324
MODELS = 10000

# 量尺、排函数的供应商、模型：配置里写 `[providers.deepseek]`、模型 `deepseek-flash`（`fake_model.home(catalog=…)`）。
BENCH_PROVIDER = "deepseek"
BENCH_MODEL = "deepseek-flash"

# 常用的几家（`resources/models/featured.toml` 的编号）：编号照它，引导里列得出来；包名照真目录里这几家的写法。
FEATURED = {
    "opencode": "@ai-sdk/openai-compatible",
    "opencode-go": "@ai-sdk/openai-compatible",
    "cline-pass": "@ai-sdk/openai-compatible",
    "deepseek": "@ai-sdk/openai-compatible",
    "anthropic": "@ai-sdk/anthropic",
    "openai": "@ai-sdk/openai",
    "openrouter": "@openrouter/ai-sdk-provider",
    "moonshotai": "@ai-sdk/openai-compatible",
    "moonshotai-cn": "@ai-sdk/openai-compatible",
    "zhipuai": "@ai-sdk/openai-compatible",
    "zai": "@ai-sdk/openai-compatible",
    "alibaba": "@ai-sdk/openai-compatible",
    "alibaba-cn": "@ai-sdk/openai-compatible",
    "minimax": "@ai-sdk/anthropic",
    "minimax-cn": "@ai-sdk/anthropic",
}

SYLLABLES = [
    "ka", "lo", "mi", "ra", "ven", "tor", "sa", "nel", "qu", "zen", "ar", "bo", "cy", "dra", "el", "fi", "gal", "ho",
    "is", "jun", "ke", "lum", "mo", "nyx", "or", "pi", "rho", "sil", "ta", "ul", "vo", "wen", "xi", "yu", "zo",
]
VARIANTS = [
    "", "", "", "-mini", "-flash", "-pro", "-lite", "-turbo", "-instruct", "-thinking", "-vl", "-coder", "-max",
    "-preview", "-air", "-nano", "-chat", "-fast", "-highspeed", "-exp",
]
WORDS = (
    "model reasoning coding agentic tasks long context multimodal vision fast cheap flagship open weights instruct "
    "tuned for tool use chat assistant latency throughput math science analysis documents images audio video "
    "general purpose efficient large small update snapshot improved stable preview release with and the of in"
).split()
EFFORTS = [
    ["low", "medium", "high", "xhigh", "max"], ["low", "medium", "high"], ["low", "high", "max"],
    ["none", "low", "medium", "high", "xhigh", "max"], ["minimal", "low", "medium", "high"], ["none", "low", "high", "max"],
    ["none", "low", "medium", "high", "max"], ["none", "high"], ["low", "medium", "high", "max"], ["high", "max"],
    ["minimal", "low", "medium", "high", "xhigh"], ["none", "minimal", "low", "medium", "high"], ["low", "high"],
]
CONTEXTS = [1000000] * 20 + [1048576] * 16 + [262144] * 11 + [1050000] * 7 + [128000] * 6 + [131072] * 5 + \
    [200000] * 5 + [400000] * 4 + [256000] * 3 + [204800] * 2 + [32768] * 2 + [0] * 2 + [65536, 163840, 2000000, 8192]
OUTPUTS = [128000] * 22 + [131072] * 14 + [65536] * 10 + [32768] * 6 + [16384] * 5 + [64000] * 4 + [262144] * 4 + \
    [8192] * 4 + [384000] * 4 + [0] * 3 + [4096] * 3 + [100000, 393216, 30000]
INPUTS = [["text"]] * 33 + [["text", "image"]] * 25 + [["text", "image", "pdf"]] * 20 + [["text", "image", "video"]] * 10 + \
    [["text", "image", "video", "audio", "pdf"]] * 4 + [["text", "image", "video", "pdf"]] * 2 + [["text", "audio"]]
KINDS = ["image"] * 18 + ["embedding"] * 8 + ["video"] * 7 + ["transcription"] * 4 + ["speech"] * 4 + ["reranking"] * 2 + \
    ["realtime"] * 2 + ["chat"]


def word(rng, low=2, high=3):
    """一个造的词：几个音节拼起来。"""
    return "".join(rng.choice(SYLLABLES) for _ in range(rng.randint(low, high)))


def price(rng, scale=1.0):
    """一个价：每一百万 token 的美元数，照真目录写成三四位有效数字的小数。"""
    value = math.exp(rng.gauss(math.log(1.2), 1.4)) * scale
    digits = max(0, 3 - int(math.floor(math.log10(value))))
    return round(value, digits) if digits else int(round(value))


def sentence(rng, mean):
    """一句说明：长度照真目录（平均 88 个字，少数两三百）。"""
    target = max(20, int(rng.expovariate(1 / mean)) + 25)
    out = []
    while sum(len(w) + 1 for w in out) < target:
        out.append(rng.choice(WORDS))
    text = " ".join(out)
    return text[0].upper() + text[1:]


def date(rng, start=2023):
    return f"{rng.randint(start, 2026)}-{rng.randint(1, 12):02d}-{rng.randint(1, 28):02d}"


class Canon:
    """一个「原型」模型：好几家列的同一个模型照它，各家只在价格、窗口、状态上有出入。"""

    def __init__(self, rng, vendor, family, name):
        self.vendor = vendor
        self.family = family if rng.random() < 0.95 else None
        self.name = name
        self.display = " ".join(part.capitalize() for part in name.replace(".", " ").split("-"))
        self.description = sentence(rng, 57)
        self.context = rng.choice(CONTEXTS)
        self.output = rng.choice(OUTPUTS)
        self.inputs = rng.choice(INPUTS)
        self.reasoning = rng.random() < 0.7
        self.tools = rng.random() < 0.85
        self.temperature = rng.random() < 0.8
        self.structured = rng.random() < 0.75
        self.open = rng.random() < 0.5
        self.knowledge = f"{rng.randint(2023, 2026)}-{rng.randint(1, 12):02d}" if rng.random() < 0.58 else None
        self.released = date(rng)
        self.updated = self.released if rng.random() < 0.7 else date(rng, int(self.released[:4]))
        self.options = self.reasoning_options(rng)
        self.interleaved = None
        roll = rng.random()
        if roll < 0.146:
            self.interleaved = {"field": "reasoning_content"}
        elif roll < 0.157:
            self.interleaved = {"field": "reasoning_details"}
        elif roll < 0.167:
            self.interleaved = True
        self.input_price = price(rng)
        self.output_price = round(self.input_price * rng.choice([2, 3, 4, 4, 5, 8]), 4)
        self.kind = rng.choice(KINDS) if rng.random() < 0.046 else None
        # 价格的写法（有哪几格、分几档）每个原型定一次：各家只差加价，写法的种数才和真目录差不多（几千种）。
        self.cost_seed = rng.random()

    def reasoning_options(self, rng):
        if not self.reasoning and rng.random() < 0.6:
            return None
        roll = rng.random()
        ladder = ["none", "minimal", "low", "medium", "high", "xhigh", "max"]
        values = rng.choice(EFFORTS) if rng.random() < 0.9 else [step for step in ladder if rng.random() < 0.5] or ["high"]
        effort = {"type": "effort", "values": values}
        toggle = {"type": "toggle"}
        budget = {"type": "budget_tokens", "min": rng.choice([128, 1024, 2048])}
        if rng.random() < 0.4:
            budget["max"] = rng.choice([32768, 65536])
        if roll < 0.61:
            return [effort]
        if roll < 0.76:
            return []
        if roll < 0.86:
            return [toggle]
        if roll < 0.92:
            return [toggle, effort]
        if roll < 0.955:
            return [toggle, budget]
        if roll < 0.975:
            return [effort, budget]
        if roll < 0.99:
            return [budget]
        return [toggle, effort, budget]


def vendors(rng):
    """造的厂家、家族、原型模型名：约 150 个家族。"""
    out = []
    seen = set()
    for _ in range(48):
        vendor = word(rng)
        while vendor in seen:
            vendor = word(rng)
        seen.add(vendor)
        families = []
        for _ in range(rng.randint(2, 5)):
            family = word(rng, 1, 2) if rng.random() < 0.5 else f"{vendor}-{word(rng, 1, 1)}"
            families.append(family)
        out.append((vendor, families))
    return out


def canons(rng, makers, count):
    """`count` 个原型：家族 + 版本 + 变体，名字不重。"""
    out = []
    names = set()
    while len(out) < count:
        vendor, families = rng.choice(makers)
        family = rng.choice(families)
        version = rng.choice(["1", "2", "2.5", "3", "3.1", "3.5", "4", "4.1", "4.5", "5", "5.1", "5.3", "6", "k2", "k3", "v4"])
        name = f"{family}-{version}{rng.choice(VARIANTS)}"
        if rng.random() < 0.08:
            name += f"-{rng.randint(2024, 2026)}{rng.randint(1, 12):02d}{rng.randint(1, 28):02d}"
        if name in names:
            continue
        names.add(name)
        out.append(Canon(rng, vendor, family, name))
    return out


def sizes(rng):
    """每家几个模型：中位 10 上下、几家聚合的五六百，合起来正好 [`MODELS`]。"""
    big = [598, 503, 468, 418, 399, 383, 216, 214, 208, 179]
    rest = [max(1, min(170, int(round(math.exp(rng.gauss(math.log(9.5), 1.2)))))) for _ in range(PROVIDERS - len(big))]
    total = sum(big) + sum(rest)
    # 照比例缩放到正好 MODELS，零头补在小的几家上。
    scale = (MODELS - sum(big)) / sum(rest)
    rest = [max(1, int(n * scale)) for n in rest]
    i = 0
    while sum(big) + sum(rest) < MODELS:
        rest[i % len(rest)] += 1
        i += 7
    while sum(big) + sum(rest) > MODELS:
        j = max(range(len(rest)), key=lambda k: rest[k])
        rest[j] -= 1
    del total
    out = big + rest
    rng.shuffle(out)
    # 量尺那一家只列三个（[`bench_models`]），多出来的给一家小的，合起来照旧是 MODELS。
    bench = list(FEATURED).index(BENCH_PROVIDER)
    spare = min(range(len(out)), key=lambda k: abs(out[k] - 50) if k != bench else MODELS)
    out[spare] += out[bench] - 3
    out[bench] = 3
    return out


def listed(rng, canon, slashed, markup):
    """一家列的一个模型：照原型，价格照这一家的加价，有时窗口、状态不同。交回 (名字, 模型的表)。"""
    name = f"{canon.vendor}/{canon.name}" if slashed else canon.name
    model = {"id": name, "name": canon.display if not slashed else f"{canon.vendor.capitalize()}: {canon.display}"}
    model["description"] = canon.description if rng.random() < 0.8 else sentence(rng, 57)
    if canon.family:
        model["family"] = canon.family
    model["attachment"] = canon.inputs != ["text"]
    model["reasoning"] = canon.reasoning
    if canon.options is not None:
        model["reasoning_options"] = canon.options
    model["tool_call"] = canon.tools
    if canon.interleaved is not None:
        model["interleaved"] = canon.interleaved
    if rng.random() < 0.99 and canon.structured:
        model["structured_output"] = rng.random() < 0.9
    if rng.random() < 0.968:
        model["temperature"] = canon.temperature
    if canon.knowledge:
        model["knowledge"] = canon.knowledge
    model["release_date"] = canon.released
    model["last_updated"] = canon.updated
    output = ["text"] if canon.kind is None else [{"image": "image", "video": "video"}.get(canon.kind, "text")]
    model["modalities"] = {"input": canon.inputs, "output": output}
    model["open_weights"] = canon.open
    context = canon.context if rng.random() < 0.85 else rng.choice(CONTEXTS)
    limit = {"context": context, "output": canon.output}
    if rng.random() < 0.186:
        limit["input"] = max(0, context - rng.choice([0, 8192, 32768, 128000]))
    model["limit"] = limit
    roll = rng.random()
    if roll < 0.086:
        model["status"] = "deprecated"
    elif roll < 0.094:
        model["status"] = "beta"
    if rng.random() < 0.955:
        model["cost"] = cost(canon, markup)
    if rng.random() < 0.861:
        model["canonical_model_id"] = f"{canon.vendor}/{canon.name}"
    if rng.random() < 0.086:
        extra = {"api": f"https://gateway.{canon.vendor}.example/v{rng.randint(1, 4)}/ai"}
        if rng.random() < 0.9:
            extra["npm"] = rng.choice(["@ai-sdk/openai", "@ai-sdk/anthropic", "@ai-sdk/openai-compatible"])
        if rng.random() < 0.16:
            extra["shape"] = "responses"
        model["provider"] = extra
    if canon.kind:
        model["type"] = canon.kind
    if rng.random() < 0.013:
        model["experimental"] = True
    return name, model


def cost(canon, markup):
    """价格：输入、输出总有，读缓存七成、写缓存两成多，一成按上下文分档，几十分之一写超过 20 万的价。写法照原型定（同一个
    原型各家写的格一样），数照这一家的加价。"""
    rng = random.Random(canon.cost_seed)
    def scaled(value):
        value = value * markup
        digits = max(0, 4 - int(math.floor(math.log10(value)))) if value > 0 else 0
        out = round(value, digits)
        return int(out) if out == int(out) else out

    free = rng.random() < 0.06
    base_in = 0 if free else scaled(canon.input_price)
    base_out = 0 if free else scaled(canon.output_price)
    out = {"input": base_in, "output": base_out}
    if rng.random() < 0.024:
        out["reasoning"] = base_out
    if rng.random() < 0.757:
        out["cache_read"] = 0 if free else scaled(canon.input_price * rng.choice([0.1, 0.1, 0.25, 0.5]))
    if rng.random() < 0.25:
        out["cache_write"] = 0 if free else scaled(canon.input_price * rng.choice([1.25, 1.25, 2]))
    if rng.random() < 0.023:
        out["input_audio"] = scaled(canon.input_price * 4)
    if rng.random() < 0.0998:
        tiers = []
        for step in range(rng.choice([1] * 14 + [2])):
            tier = {"input": scaled(canon.input_price * (2 + step)), "output": scaled(canon.output_price * (1.5 + step))}
            if rng.random() < 0.87:
                tier["cache_read"] = scaled(canon.input_price * (0.2 + step / 10))
            if rng.random() < 0.5:
                tier["cache_write"] = scaled(canon.input_price * (2.5 + step))
            tier["tier"] = {"type": "context", "size": [200000, 272000, 128000][step % 3] * (step + 1)}
            tiers.append(tier)
        out["tiers"] = tiers
    if rng.random() < 0.079:
        over = {"input": scaled(canon.input_price * 2), "output": scaled(canon.output_price * 1.5)}
        if rng.random() < 0.93:
            over["cache_read"] = scaled(canon.input_price * 0.2)
        if rng.random() < 0.55:
            over["cache_write"] = scaled(canon.input_price * 2.5)
        out["context_over_200k"] = over
    return out


def bench_models():
    """量尺、排函数那一家的几个模型：照真目录同名那一条的样子手写，底栏照它算花费。"""
    def one(name, display, context, output, cost, inputs):
        return {
            "id": name, "name": display,
            "description": "Bench model written by bench/catalog.py with the same shape as the catalog entries",
            "family": "deepseek-flash", "attachment": "image" in inputs, "reasoning": True,
            "reasoning_options": [{"type": "toggle"}, {"type": "effort", "values": ["low", "high", "max"]}],
            "tool_call": True, "interleaved": {"field": "reasoning_content"}, "structured_output": True,
            "temperature": True, "knowledge": "2025-05", "release_date": "2026-10-17", "last_updated": "2026-10-17",
            "modalities": {"input": inputs, "output": ["text"]}, "open_weights": True,
            "limit": {"context": context, "output": output}, "cost": cost,
            "canonical_model_id": f"deepseek/{name}",
        }

    flash = {"input": 0.15, "output": 0.6, "reasoning": 0.6, "cache_read": 0.003}
    return {
        BENCH_MODEL: one(BENCH_MODEL, "Bench Flash", 1048576, 393216, flash, ["text", "image"]),
        "deepseek-v4-flash": one("deepseek-v4-flash", "Bench Flash Older", 1000000, 393216, flash, ["text"]),
        "deepseek-v4-pro": one("deepseek-v4-pro", "Bench Pro", 1048576, 393216,
                               {"input": 0.6, "output": 2.4, "cache_read": 0.012}, ["text"]),
    }


def catalog():
    """整份目录：编号 → 供应商，照编号排（真目录也是照编号排的）。"""
    rng = random.Random(SEED)
    makers = vendors(rng)
    pool = canons(rng, makers, 3100)
    # 原型的热门程度：前面的几十家都列（Zipf），后面的只一两家列。
    weights = [1 / (rank + 8) ** 0.9 for rank in range(len(pool))]
    counts = sizes(rng)
    ids = list(FEATURED)
    seen = set(ids)
    while len(ids) < PROVIDERS:
        pid = word(rng) + rng.choice(["", "", "-ai", "-cloud", "-cn", "-api"])
        if pid not in seen:
            seen.add(pid)
            ids.append(pid)
    out = {}
    for pid, count in zip(ids, counts):
        # 大的几家多半是聚合的：名字写成 `<厂>/<名字>`。
        slashed = count >= 150 or rng.random() < 0.35
        npm = FEATURED.get(pid) or rng.choices(
            ["@ai-sdk/openai-compatible", "@ai-sdk/anthropic", "@ai-sdk/openai", f"@{pid}/ai-sdk-provider"],
            [88, 3, 2, 7],
        )[0]
        title = " ".join(part.capitalize() for part in pid.split("-"))
        provider = {"id": pid, "env": [pid.upper().replace("-", "_") + "_API_KEY"]}
        if rng.random() < 0.04:
            provider["env"].append(pid.upper().replace("-", "_") + "_BASE_URL")
        provider["npm"] = npm
        if rng.random() < 0.923 and pid not in ("anthropic", "openai"):
            provider["api"] = f"https://api.{pid}.example/v{rng.randint(1, 3)}"
        provider["name"] = title
        provider["doc"] = f"https://docs.{pid}.example/{rng.choice(['models', 'api/pricing', 'reference/chat'])}"
        if pid == BENCH_PROVIDER:
            provider["models"] = bench_models()
            out[pid] = provider
            continue
        markup = rng.choice([1] * 10 + [1.1, 1.25])
        models = {}
        while len(models) < count:
            canon = rng.choices(pool, weights)[0] if rng.random() < 0.6 else rng.choice(pool)
            name, model = listed(rng, canon, slashed, markup)
            if name not in models:
                models[name] = model
        provider["models"] = models
        out[pid] = provider
    return dict(sorted(out.items()))


def text():
    """整份目录的原文：不缩进，同真目录。"""
    return json.dumps(catalog(), ensure_ascii=False, separators=(",", ":"))


def main():
    body = text()
    if len(sys.argv) > 1:
        with open(sys.argv[1], "w") as out:
            out.write(body)
    else:
        sys.stdout.write(body)


if __name__ == "__main__":
    main()
