#!/usr/bin/env python3
import argparse
import json
import math
import os
import ssl
import threading
import time
import urllib.error
import urllib.request
from collections import Counter
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from http.cookies import SimpleCookie

import requests


TARGETS = {
    "4c": "https://mem012.0120012.xyz",
    "1c1g": "https://memory.0120012.xyz",
}


def build_tls_context() -> tuple[ssl.SSLContext, str]:
    # What：从显式配置或常见系统路径加载可信 CA，并返回 context 与 bundle 路径。
    # Why：Python 自带 CA 路径可能未初始化，不能因此关闭生产域名的 TLS 验证。
    candidates = [
        os.getenv("SSL_CERT_FILE"),
        os.getenv("REQUESTS_CA_BUNDLE"),
        os.getenv("CURL_CA_BUNDLE"),
        "/etc/ssl/cert.pem",
        "/etc/ssl/certs/ca-certificates.crt",
        requests.certs.where(),
    ]
    for ca_file in candidates:
        if ca_file and os.path.isfile(ca_file):
            return ssl.create_default_context(cafile=ca_file), ca_file
    raise RuntimeError("找不到可用的 CA bundle")


TLS_CONTEXT, TLS_CA_FILE = build_tls_context()
THREAD_LOCAL = threading.local()


def get_session() -> requests.Session:
    # What：为每个工作线程创建并复用独立的 HTTP Session。
    # Why：共享 Session 非线程安全，而每请求新建 TLS 连接会扭曲压测结果。
    session = getattr(THREAD_LOCAL, "session", None)
    if session is None:
        session = requests.Session()
        session.verify = TLS_CA_FILE
        session.headers.update({"User-Agent": "mem012-load-test/1"})
        THREAD_LOCAL.session = session
    return session


def login(base_url: str, api_token: str, timeout: float = 10.0) -> str:
    # What：用 API token 换取压测读取接口所需的 mem_session Cookie。
    # Why：原始密钥不能进入并发任务、结果文件或错误样本。
    if not api_token:
        raise ValueError("MEM012_API_TOKEN 不能为空")
    request = urllib.request.Request(
        f"{base_url.rstrip('/')}/api/auth/verify",
        data=json.dumps({"key": api_token}).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "mem012-load-test/1"},
        method="POST",
    )
    try:
        response = urllib.request.urlopen(request, timeout=timeout, context=TLS_CONTEXT)
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"登录失败: HTTP {error.code}") from error
    except (urllib.error.URLError, TimeoutError) as error:
        raise RuntimeError(f"登录失败: {error}") from error
    with response:
        cookies = SimpleCookie()
        for header in response.headers.get_all("Set-Cookie", []):
            cookies.load(header)
    session = cookies.get("mem_session")
    if session is None or not session.value:
        raise RuntimeError("登录响应缺少 mem_session Cookie")
    return f"mem_session={session.value}"


def probe(
    base_url: str,
    path: str = "/api/health",
    timeout: float = 10.0,
    headers: dict[str, str] | None = None,
    deadline: float | None = None,
) -> dict | None:
    # What：向单个目标发送一次只读请求，并返回结构化状态与延迟。
    # Why：正式升压前必须先证明 DNS、TLS、反代和响应状态都正常。
    url = f"{base_url.rstrip('/')}/{path.lstrip('/')}"
    request_headers = {**(headers or {}), "User-Agent": "mem012-load-test/1"}
    if deadline is not None:
        remaining = deadline - time.perf_counter()
        if remaining <= 0:
            return None
        timeout = min(timeout, remaining)
    started = time.perf_counter()
    try:
        response = get_session().get(
            url, headers=request_headers, timeout=timeout, verify=TLS_CA_FILE
        )
        ok = 200 <= response.status_code < 300
        return {
            "ok": ok,
            "status": response.status_code,
            "latency_ms": round((time.perf_counter() - started) * 1000, 3),
            "bytes": len(response.content),
            "error": None if ok else f"HTTP {response.status_code}",
        }
    except requests.RequestException as error:
        return {
            "ok": False,
            "status": None,
            "latency_ms": round((time.perf_counter() - started) * 1000, 3),
            "bytes": 0,
            "error": str(error),
        }


def run_stage(
    base_url: str,
    path: str,
    concurrency: int,
    duration_seconds: float,
    timeout: float,
    headers: dict[str, str] | None = None,
) -> dict:
    # What：按固定并发执行只读请求，并汇总吞吐、延迟和状态码。
    # Why：两个服务器必须使用同一计算口径，结果才可以直接比较。
    if concurrency < 1 or duration_seconds <= 0 or timeout <= 0:
        raise ValueError("concurrency、duration_seconds 和 timeout 必须大于 0")
    started = time.perf_counter()
    deadline = started + duration_seconds
    results = []

    # What：提交与阶段截止时间绑定的单次探测请求。
    # Why：最后一批无响应请求不能按默认超时拖长已结束的压测阶段。
    def submit_probe(executor: ThreadPoolExecutor):
        if time.perf_counter() >= deadline:
            return None
        return executor.submit(probe, base_url, path, timeout, headers, deadline)

    with ThreadPoolExecutor(max_workers=concurrency) as executor:
        futures = {future for _ in range(concurrency) if (future := submit_probe(executor))}
        while futures:
            done, futures = wait(futures, return_when=FIRST_COMPLETED)
            results.extend(result for future in done if (result := future.result()) is not None)
            futures.update(
                future for _ in done if (future := submit_probe(executor))
            )
    duration = time.perf_counter() - started
    if not results:
        raise RuntimeError("压测阶段未完成任何请求")
    request_count = len(results)
    latencies = sorted(result["latency_ms"] for result in results)
    percentile = lambda value: latencies[
        min(len(latencies) - 1, math.ceil(len(latencies) * value) - 1)
    ]
    successes = sum(result["ok"] for result in results)
    statuses = Counter(str(result["status"]) for result in results)
    error_samples = list(
        dict.fromkeys(result["error"] for result in results if result["error"])
    )[:3]
    return {
        "requests": request_count,
        "successes": successes,
        "error_rate": round((request_count - successes) / request_count, 6),
        "duration_s": round(duration, 3),
        "rps": round(request_count / duration, 3),
        "latency_ms": {
            "average": round(sum(latencies) / len(latencies), 3),
            "p50": percentile(0.50),
            "p95": percentile(0.95),
            "p99": percentile(0.99),
            "max": latencies[-1],
        },
        "statuses": dict(sorted(statuses.items())),
        "error_samples": error_samples,
    }


def main() -> int:
    # What：解析安全的压测参数，并按目标逐个执行同一阶段。
    # Why：阶段持续时间必须由调用者显式指定，避免意外产生压测流量。
    parser = argparse.ArgumentParser(description="mem012 只读压测器")
    parser.add_argument("--target", choices=["4c", "1c1g", "all"], default="all")
    parser.add_argument(
        "--path", choices=["/", "/api/health", "/api/memories"], default="/api/health"
    )
    parser.add_argument("--concurrency", type=int, default=1)
    parser.add_argument("--duration", type=float, required=True)
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("--project", default="loadtest")
    args = parser.parse_args()
    if args.concurrency < 1 or args.duration <= 0 or args.timeout <= 0:
        parser.error("concurrency、duration 和 timeout 必须大于 0")
    selected = TARGETS if args.target == "all" else {args.target: TARGETS[args.target]}
    failed = False
    for target_name, target_url in selected.items():
        headers = None
        try:
            if args.path == "/api/memories":
                token = os.getenv(f"MEM012_API_TOKEN_{target_name.upper()}") or os.getenv(
                    "MEM012_API_TOKEN"
                )
                headers = {
                    "Cookie": login(target_url, token or "", args.timeout),
                    "X-Mem-Project": args.project,
                }
            result = run_stage(
                target_url, args.path, args.concurrency, args.duration, args.timeout, headers
            )
            failed |= result["error_rate"] > 0
            print(json.dumps({"target": target_name, "url": target_url, **result}))
        except (ValueError, RuntimeError) as error:
            failed = True
            print(json.dumps({"target": target_name, "url": target_url, "error": str(error)}))
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
