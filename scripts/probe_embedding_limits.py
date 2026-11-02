#!/usr/bin/env python3
"""核实固定版本 tokenizer；加 --live 检查服务计数与 8192/8193 token 边界。"""

import argparse
from datetime import datetime, timezone
import hashlib
from importlib.metadata import version
import json
import os
from pathlib import Path
import tempfile
import time
import tomllib

import requests
from tokenizers import Tokenizer

from embedding_p0 import ModelClient, ROOT
from load_embedding_samples import load_samples

REVISIONS = {
    "embeddings": ("BAAI/bge-m3", "e44369c5623cc146f016da906583db4ee0e3488d"),
    "rerank": ("BAAI/bge-reranker-v2-m3", "e099d4b9cdbd291b1569d416f19aaa6523570bc3"),
}
ARTIFACT_HASHES = {
    "BAAI/bge-m3": {
        "tokenizer.json": "21106b6d7dab2952c1d496fb21d5dc9db75c28ed361a05f5020bbba27810dd08",
        "tokenizer_config.json": "a62b2b6784f990259fddef5f16388693a8043be4f69179e6a5257eeb3f9abac4",
        "config.json": "26159e7ad065073448460117eb24b7a4572f6f4e78eadff65dc0a11c052449fa",
    },
    "BAAI/bge-reranker-v2-m3": {
        "tokenizer.json": "69564b696052886ed0ac63fa393e928384e0f8caada38c1f4864a9bfbf379c15",
        "tokenizer_config.json": "7e4c1cc848840aeccdd763458c18dd525eb0f795c992e00ebe9c28554e7db2d4",
        "config.json": "13dcd6c31d9fec9d1d8e158702072f62d7fa7d312a64b9fe057bec9a08cfe41a",
    },
}


def tokenizer_artifacts(model, revision):
    # What：仅下载官方固定提交的 tokenizer 与配置，记录文件 hash。
    # Why：不下载模型权重，仍可重复验证特殊 token 开销和真实文本预算。
    directory = Path(tempfile.gettempdir()) / "mem012-p0-tokenizers" / revision
    directory.mkdir(parents=True, exist_ok=True)
    records = {}
    for name in ("tokenizer.json", "tokenizer_config.json", "config.json"):
        path = directory / name
        url = f"https://modelscope.cn/models/{model}/resolve/{revision}/{name}"
        if not path.exists():
            for attempt in range(3):
                try:
                    response = requests.get(url, timeout=(15, 45))
                    response.raise_for_status()
                    data = response.content
                    json.loads(data)
                    path.write_bytes(data)
                    break
                except (requests.RequestException, ValueError):
                    if attempt == 2:
                        raise
                    time.sleep(1)
        data = path.read_bytes()
        if hashlib.sha256(data).hexdigest() != ARTIFACT_HASHES[model][name]:
            raise ValueError(f"官方元数据与下载文件 hash 不一致：{model}/{name}")
        records[name] = {"url": url, "sha256": hashlib.sha256(data).hexdigest(), "bytes": len(data)}
        print(f"已核对 {model}/{name}", flush=True)
    tokenizer = Tokenizer.from_file(str(directory / "tokenizer.json"))
    tokenizer.no_truncation()
    tokenizer.no_padding()
    settings = json.loads((directory / "tokenizer_config.json").read_text())
    config = json.loads((directory / "config.json").read_text())
    return tokenizer, {"model": model, "revision": revision, "repository": f"https://modelscope.cn/models/{model}",
        "hash_source": f"https://modelscope.cn/api/v1/models/{model}/repo/files?Revision={revision}&Recursive=false",
        "artifacts": records,
        "tokenizer_class": settings.get("tokenizer_class"), "model_max_length": settings.get("model_max_length"),
        "max_position_embeddings": config.get("max_position_embeddings"),
        "single_special_tokens": tokenizer.num_special_tokens_to_add(False),
        "pair_special_tokens": tokenizer.num_special_tokens_to_add(True)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--live", action="store_true", help="额外调用模型，核对实际计数和边界行为")
    parser.add_argument("--report", type=Path, default=ROOT / "docs/embedding_p0_model_limits.json")
    args = parser.parse_args()
    report = {"recorded_at": datetime.now(timezone.utc).isoformat(), "tokenizers_version": version("tokenizers"),
              "user_supplied": {"embedding_dimension": 1024, "embedding_context_tokens": 8192,
                                "rerank_context_label": "8K", "max_rpm": 2000, "max_tpm": 500000,
                                "quota_scope": "用户提供一组模型上限；本轮保守共用，不代表分别核验了两模型账户配额"},
              "models": {}, "service_checks": []}
    samples = load_samples(ROOT / "docs/embedding_search_samples.json")
    tokenizers = {}
    for section, (model, revision) in REVISIONS.items():
        tokenizer, row = tokenizer_artifacts(model, revision)
        tokenizers[section] = tokenizer
        row["examples"] = [{"text": text, "characters": len(text), "tokens_with_specials": len(tokenizer.encode(text).ids)}
            for text in ("网页导出", "Export web pages to Markdown.", "def export_page(url):\n    return url\n")]
        row["max_fixture_content_tokens"] = max(len(tokenizer.encode(memory["content"]).ids) for memory in samples["memories"])
        row["max_fixture_include_tokens"] = max(len(tokenizer.encode(" ".join(word.strip() for word in case["request"]["params"]["terms"]["include"])).ids)
            for case in samples["cases"])
        report["models"][section] = row
    if args.live:
        config = tomllib.loads(Path(os.environ.get("MEM012_CONFIG", ROOT / "config.toml")).read_text())
        client = ModelClient(config)
        for section, (model, _) in REVISIONS.items():
            tokenizer = tokenizers[section]
            assert config[section][section + "_model"] == model, "模型变更后需重新固定 tokenizer revision"
            for target in (None, 8192, 8193):
                query = "网页导出"
                if section == "embeddings":
                    text = query if target is None else ("a " * (target - tokenizer.num_special_tokens_to_add(False))).rstrip()
                    tokens = len(tokenizer.encode(text).ids)
                    body = {"model": model, "input": [text]}
                else:
                    text = "将网页导出为 Markdown。" if target is None else ("a " * (
                        target - len(tokenizer.encode(query, add_special_tokens=False).ids) - tokenizer.num_special_tokens_to_add(True))).rstrip()
                    tokens = len(tokenizer.encode(query, text).ids)
                    body = {"model": model, "query": query, "documents": [text], "top_n": 1}
                assert target is None or tokens == target, "构造的边界文本 token 数不符"
                response = client.post(section, body)
                result = {"model_type": section, "local_input_tokens_with_specials": tokens,
                    "boundary_target": target, "call": client.calls[-1], "result_count": None}
                if response is not None:
                    result["result_count"] = len(response.get("data", response.get("results", [])))
                    if section == "embeddings":
                        result["dimensions"] = len(response["data"][0]["embedding"])
                report["service_checks"].append(result)
                print(f"已测 {section}: local_tokens={tokens}, HTTP={result['call']['http_status']}", flush=True)
    report["limitations"] = ["用户提供的窗口和配额单列来源，不将缺少限流头当作无限配额。",
        "HTTP 200 不能证明服务没有截断或内部切块；边界探测不替代本地完整覆盖和 token 预算。",
        "rerank 的 query/document 联合计数包含 pair 特殊 token；服务未返回 token usage 时，计费 token 数保持 unknown。"]
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(f"已覆盖写入 {args.report}", flush=True)


if __name__ == "__main__":
    main()
