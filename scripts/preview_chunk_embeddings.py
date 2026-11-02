#!/usr/bin/env python3
"""需要 Python 3.11+ 和 requests；读取 test.txt，生成向量并重排，结果写入同目录 rag_result.txt。

python3 scripts/preview_chunk_embeddings.py
python3 scripts/preview_chunk_embeddings.py --full
python3 scripts/preview_chunk_embeddings.py --query '归档前需要做什么？'
"""
import argparse
import json
import math
import os
import re
import tomllib
from pathlib import Path

import requests


def embed_chunks(chunks, settings):
    # What：批量生成并校验每个 chunk 对应的独立向量。
    # Why：按 API 索引保持文本与向量对应，避免把多块合成后丢失局部信息。
    if not chunks or any(not chunk.strip() for chunk in chunks):
        raise ValueError("至少需要一个非空 chunk，请在 test.txt 中填写文本并以空行分段")
    body = {"model": settings["embeddings_model"], "input": chunks}
    headers = {"Authorization": "Bearer " + settings["embeddings_key"].strip()}
    response = requests.post(settings["embeddings_api"].rstrip("/") + "/embeddings", json=body, headers=headers, timeout=(10, 40))
    response.raise_for_status()
    rows = response.json()["data"]
    if len(rows) != len(chunks) or {row["index"] for row in rows} != set(range(len(chunks))):
        raise ValueError("API 返回的 chunk 索引缺失、重复或越界")
    for row in rows:
        vector = row["embedding"]
        if len(vector) != settings["embeddings_dimension"] or not all(math.isfinite(value) for value in vector):
            raise ValueError("API 返回的向量维度不符或含非有限数值")
    return sorted(rows, key=lambda row: row["index"])


def rerank_chunks(query, chunks, settings):
    # What：请求相关性分数并按分数重排全部 chunk。
    # Why：验证索引和分数后再关联向量，避免部分响应导致文本与向量错配。
    body = {"model": settings["rerank_model"], "query": query.strip(), "documents": chunks, "top_n": len(chunks)}
    headers = {"Authorization": "Bearer " + settings["rerank_key"].strip()}
    response = requests.post(settings["rerank_api"].rstrip("/") + "/rerank", json=body, headers=headers, timeout=(10, 40))
    response.raise_for_status()
    ranked = response.json()["results"]
    if len(ranked) != len(chunks) or {item["index"] for item in ranked} != set(range(len(chunks))):
        raise ValueError("rerank 返回的 chunk 索引缺失、重复或越界")
    for item in ranked:
        score = item.get("relevance_score", item.get("score"))
        if type(item["index"]) is not int or type(score) not in (int, float) or not math.isfinite(score):
            raise ValueError("rerank 返回的索引或分数无效")
        item["relevance_score"] = score
    return sorted(ranked, key=lambda item: item["relevance_score"], reverse=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--full", action="store_true", help="输出完整向量，默认只展示前 8 维")
    parser.add_argument("--query", default="将网页转换为 Markdown，并保留代码块", help="rerank 查询文本")
    args = parser.parse_args()
    if not args.query.strip():
        parser.error("--query 不能为空")
    try:
        text = Path(__file__).with_name("test.txt").read_text(encoding="utf-8")
        chunks = [part.strip() for part in re.split(r"\n\s*\n", text.strip()) if part.strip()]
        path = Path(os.environ.get("MEM012_CONFIG", Path(__file__).resolve().parents[1] / "config.toml"))
        config = tomllib.loads(path.read_text(encoding="utf-8"))
        settings = config["embeddings"]
        rows = embed_chunks(chunks, settings)
        for row in rows:
            row["chunk"] = chunks[row["index"]]
            row["dimensions"] = len(row["embedding"])
            if not args.full:
                row["embedding_preview"] = row.pop("embedding")[:8]
        if config["rerank"]["enabled"]:
            ranked = rerank_chunks(args.query, chunks, config["rerank"])
            rows = [{**rows[item["index"]], "rerank_score": item["relevance_score"]} for item in ranked]
        result = {"model": settings["embeddings_model"], "query": args.query.strip(), "rerank_enabled": config["rerank"]["enabled"], "chunks": rows}
        output = json.dumps(result, ensure_ascii=False, indent=2)
        Path(__file__).with_name("rag_result.txt").write_text(output + "\n", encoding="utf-8")
        print(output)
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"{type(error).__name__}: {error}\n")
