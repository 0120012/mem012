#!/usr/bin/env python3
"""加载并展开 P0 离线样本：python3 scripts/load_embedding_samples.py [样本路径]。"""

import argparse
import json
from pathlib import Path


def load_samples(path):
    # What：展开样本正文，并补齐记忆和检索用例的缺省 profile。
    # Why：长文末尾与跨 profile 隔离必须基于完整语料验证，不能直接检索占位符。
    samples = json.loads(Path(path).read_text(encoding="utf-8"))
    replacements = {}
    for marker, recipe in samples["content_expansions"].items():
        text, repeat = recipe["text"], recipe["repeat"]
        if not marker or not isinstance(text, str) or type(repeat) is not int or repeat < 0:
            raise ValueError("正文展开规则必须包含非空标记、文本和非负整数 repeat")
        replacements[marker] = text * repeat
    for memory in samples["memories"]:
        for marker, text in replacements.items():
            memory["content"] = memory["content"].replace(marker, text)
        memory.setdefault("profile", samples["profile"])
    for case in samples["cases"]:
        case.setdefault("profile", samples["profile"])
    return samples


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("path", nargs="?", type=Path, default=Path(__file__).resolve().parents[1] / "docs/embedding_search_samples.json")
    args = parser.parse_args()
    try:
        samples = load_samples(args.path)
        summary = {"memories": len(samples["memories"]), "cases": len(samples["cases"])}
        summary["splits"] = {name: sum(case["split"] == name for case in samples["cases"]) for name in ("tune", "holdout", "contract")}
        summary["max_content_chars"] = max(len(memory["content"]) for memory in samples["memories"])
        print(json.dumps(summary, ensure_ascii=False, indent=2))
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(1, f"{type(error).__name__}: {error}\n")
