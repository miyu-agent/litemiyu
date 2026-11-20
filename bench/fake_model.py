"""本机的假模型服务器：照 OpenAI Chat 的流式写法回一句话（量尺、排函数的「发第一句话」那一段用，docs/00-设计.md 第七节）。

只用标准库。起在 127.0.0.1 的一个空端口上，在自己的线程里跑；每个请求都回同一句话。
不连外网：数据目录、配置都在调用方给的临时目录里（`config`）。

也能起成 HTTPS 的（`start_tls`，占用 四）：真用的人连供应商走的都是 HTTPS，TLS 那一路的代码、证书库、留着的加密连接量尺里
没有。证书是现造的：临时目录里一个只活两天的 CA，签一张 `127.0.0.1` 的证书（要 `openssl` 命令）；程序照 `SSL_CERT_FILE` 信任
「系统的证书库 + 这个 CA」（`rustls-native-certs` 认这个环境变量，webpki 自带的那一份照旧），和真用的人一样系统的那一批根都要读。

数据目录有两种（LM-9补，`home`）：
- 没有目录（`catalog` 不给）：供应商 `fake` 指向 `127.0.0.1` 的假服务器，地址在本机，程序照「本机的服务」算（价格当 0、不取目录），
  缓存里没有模型目录。首次使用还没取到目录、只用本机模型的人是这个样子。
- 缓存里有目录（`catalog` 给目录的原文，量尺、排函数用 `catalog.py` 造的那一份）：用户平常的样子。缓存目录里放
  `models/models-dev.json` 和它的 `models-dev.meta.json`（刚取过），供应商写目录里认得的那一家（`catalog.BENCH_PROVIDER`，模型
  `catalog.BENCH_MODEL`），地址指向 `127.0.0.2` 的假服务器：在本机的回环上、但不是程序认的「本机地址」（`127.0.0.1`、`localhost`、
  `::1`），照网上的供应商走——认目录里的那一家、查窗口和价格、底栏写花费，同真用的人。`[models.catalog]` 的地址指到 `127.0.0.1:9`
  （没人听）：缓存是刚取的，不到点不取；万一取了也连不出本机。
"""

import http.server
import json
import os
import ssl
import subprocess
import threading
import time

import catalog as fake_catalog

ANSWER = ["好的，", "这是本机假服务器的回答。"]

# 收到请求以后等几秒再回（排函数的剧本设，量尺不设）：模型真回话总要等一会儿，等着的那几帧（转圈、计时、闪的字）才画得出来；
# 立刻回的话，模拟器里有时一帧都没画就回完了，那几个函数时录时不录，清单跟着变（占用 四）。
DELAY = [0.0]

# 收到过几个请求（这个进程里所有假服务器合计）：等回答的一方照它认出「回车以后才来的」回答。
_SERVED = [0]
_SERVED_LOCK = threading.Lock()


def served():
    """到这一刻收到过几个请求。"""
    with _SERVED_LOCK:
        return _SERVED[0]


def _stream():
    """一次回答的 SSE：几段字，最后报用量、`[DONE]`。"""
    events = [
        {"choices": [{"index": 0, "delta": {"role": "assistant", "content": piece}, "finish_reason": None}]}
        for piece in ANSWER
    ]
    events.append(
        {
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 120, "completion_tokens": 12, "total_tokens": 132},
        }
    )
    body = "".join(f"data: {json.dumps(event, ensure_ascii=False)}\n\n" for event in events)
    return (body + "data: [DONE]\n\n").encode()


class _Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_POST(self):  # noqa: N802（标准库的名字）
        with _SERVED_LOCK:
            _SERVED[0] += 1
        length = int(self.headers.get("Content-Length") or 0)
        self.rfile.read(length)
        time.sleep(DELAY[0])
        body = _stream()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        """不往终端打每个请求。"""


# 缓存里有目录的那种数据目录，假服务器听的地址：回环上，但不是程序认的「本机地址」（见开头）。
REMOTE = "127.0.0.2"


def start(host="127.0.0.1"):
    """起服务器（听 `host`），交回 `http://<host>:<端口>/v1`。进程退出时跟着停。"""
    server = http.server.ThreadingHTTPServer((host, 0), _Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return f"http://{host}:{server.server_address[1]}/v1"


# 系统的证书库（Arch、Debian 系、Fedora 系的合在一起的那一份放在这几处，有哪个用哪个）。
SYSTEM_BUNDLES = (
    "/etc/ssl/certs/ca-certificates.crt",
    "/etc/pki/tls/certs/ca-bundle.crt",
    "/etc/ssl/cert.pem",
)


def _openssl(*args, cwd):
    subprocess.run(["openssl", *args], cwd=cwd, check=True, capture_output=True)


def certificates(root):
    """在 `root` 下现造一个临时 CA 和它签的 `127.0.0.1`、`127.0.0.2` 的证书（P-256，两天就过期），交回 (证书, 私钥, 信任的证书文件)：
    信任的那一份是系统的证书库再加这个 CA。"""
    os.makedirs(root, exist_ok=True)
    curve = ("-newkey", "ec", "-pkeyopt", "ec_paramgen_curve:P-256", "-nodes")
    _openssl(
        "req", "-x509", *curve, "-keyout", "ca.key", "-out", "ca.pem", "-days", "2",
        "-subj", "/CN=LiteMiyu bench CA",
        "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign",
        cwd=root,
    )
    _openssl("req", *curve, "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=127.0.0.1", cwd=root)
    with open(os.path.join(root, "server.ext"), "w") as out:
        out.write(f"basicConstraints=CA:FALSE\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1,IP:{REMOTE}\n")
    _openssl(
        "x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial",
        "-out", "server.pem", "-days", "2", "-extfile", "server.ext",
        cwd=root,
    )
    trusted = os.path.join(root, "trusted.pem")
    with open(trusted, "w") as out:
        for bundle in SYSTEM_BUNDLES:
            if os.path.exists(bundle):
                with open(bundle) as system:
                    out.write(system.read().rstrip("\n") + "\n")
                break
        with open(os.path.join(root, "ca.pem")) as ca:
            out.write(ca.read())
    return os.path.join(root, "server.pem"), os.path.join(root, "server.key"), trusted


def start_tls(root, host="127.0.0.1"):
    """起 HTTPS 的服务器（听 `host`），证书现造在 `root` 下（[`certificates`]）。交回 (`https://<host>:<端口>/v1`, 给程序信任的
    证书文件)。进程退出时跟着停。"""
    cert, key, trusted = certificates(root)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    context.set_alpn_protocols(["http/1.1"])
    server = http.server.ThreadingHTTPServer((host, 0), _Handler)
    server.socket = context.wrap_socket(server.socket, server_side=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return f"https://{host}:{server.server_address[1]}/v1", trusted


# 缓存里有目录时底栏的窗口那一段（`132/1M(0%)`：量尺那一家的模型窗口 1048576）。量尺、排函数说完一句以后等它画出来，认不出
# 目录里那一家（窗口、价格查不到）的当场报错，不悄悄量成没有目录的样子。
WINDOW = "/1M(".encode()

_CATALOG = []


def generated():
    """`catalog.py` 造的那一份目录的原文（这个进程里只造一次）。"""
    if not _CATALOG:
        _CATALOG.append(fake_catalog.text())
    return _CATALOG[0]


def home(root, base_url, trusted=None, catalog=None):
    """在 `root` 下面摆好一份数据目录（`LITEMIYU_HOME`）：供应商指向假服务器。交回要给程序的环境变量。连 HTTPS 的（`trusted`
    是 [`start_tls`] 交回的证书文件）照 `SSL_CERT_FILE` 信任它。`catalog` 是目录的原文：给了的缓存里放一份（刚取过），供应商写
    目录里认得的那一家（开头「数据目录有两种」）；地址要用 [`REMOTE`] 上的假服务器，不然照本机的服务算、查不到价格。"""
    config = os.path.join(root, "config")
    os.makedirs(config, exist_ok=True)
    with open(os.path.join(config, "config.toml"), "w") as out:
        if catalog is None:
            out.write(
                f'[providers.fake]\ndriver = "openai-chat"\nbase_url = "{base_url}"\nkey = {{ env = "LITEMIYU_BENCH_KEY" }}\n\n'
                '[providers.fake.models."m1"]\n\n[models]\nchat = "fake/m1"\n'
            )
        else:
            provider, model = fake_catalog.BENCH_PROVIDER, fake_catalog.BENCH_MODEL
            out.write(
                f'[providers.{provider}]\nbase_url = "{base_url}"\nkey = {{ env = "LITEMIYU_BENCH_KEY" }}\n\n'
                f'[providers.{provider}.models."{model}"]\n\n[models]\nchat = "{provider}/{model}"\n\n'
                '[models.catalog]\nurl = "http://127.0.0.1:9/api.json"\n'
            )
    if catalog is not None:
        # 照程序读缓存的路径：`<LITEMIYU_HOME>/cache/models/models-dev.json`（`Paths::model_cache`、`models::catalog::FILE`），
        # 旁边的 meta 记着刚取过（`fetched` 照程序的写法：UTC、毫秒、`Z`）。
        cache = os.path.join(root, "cache", "models")
        os.makedirs(cache, mode=0o700, exist_ok=True)
        with open(os.path.join(cache, "models-dev.json"), "w") as out:
            out.write(catalog)
        now = time.time()
        fetched = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(now)) + f".{int(now * 1000) % 1000:03d}Z"
        with open(os.path.join(cache, "models-dev.meta.json"), "w") as out:
            json.dump({"source": "https://models.dev/api.json", "fetched": fetched, "etag": '"bench"'}, out)
    # 家目录也放在临时目录里：被测程序不碰真的家目录（审计 #070，`pty_run.spawn` 不再带宿主的 `HOME`）。
    home = os.path.join(root, "home")
    os.makedirs(home, exist_ok=True)
    env = {"LITEMIYU_HOME": root, "LITEMIYU_BENCH_KEY": "sk-bench", "HOME": home}
    if trusted:
        env["SSL_CERT_FILE"] = trusted
    return env
