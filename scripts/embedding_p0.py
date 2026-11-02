#!/usr/bin/env python3
"""P0 样本自检；加 --live 用 Python 执行现有搜索 SQL，覆盖写入参考基线报告。"""

import argparse
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import subprocess
import time
import tomllib

import requests

from load_embedding_samples import load_samples

ROOT = Path(__file__).resolve().parents[1]
FIELDS = ("title", "summary", "keywords", "content", "recall_when")


def normalize_request(params):
    # What：解释冻结的目标参数契约，供标注和结果判分使用。
    # Why：独立于旧生产解析器，才能发现旧行为与目标契约的差异。
    if not isinstance(params, dict) or set(params) - {"terms", "filters", "limit"}:
        raise ValueError("未知搜索参数")
    terms, filters = params.get("terms"), params.get("filters")
    if not isinstance(terms, dict) or set(terms) != {"include", "exclude"}:
        raise ValueError("terms 必须包含 include 和 exclude")
    for values in terms.values():
        if not isinstance(values, list) or any(not isinstance(word, str) or not word.strip() for word in values):
            raise ValueError("关键词必须为非空白字符串数组")
    if not terms["include"] and not terms["exclude"]:
        raise ValueError("include/exclude 不能同时为空")
    if not isinstance(filters, list) or any(not isinstance(field, str) or field not in FIELDS for field in filters):
        raise ValueError("filters 不合法")
    limit = params.get("limit")
    if limit is not None and (type(limit) is not int or not 1 <= limit <= 2147483647):
        raise ValueError("limit 必须为正的 int32")
    include = [word.strip() for word in terms["include"]]
    exclude = [word.strip() for word in terms["exclude"]]
    return {"include": include, "exclude": exclude, "include_text": " ".join(include),
            "fields": [field for field in FIELDS if not filters or field in filters],
            "limit": min(limit or 8, 8)}


def hard_matches(samples, case):
    plan = normalize_request(case["request"]["params"])
    matches = set()
    for memory in samples["memories"]:
        if memory["profile"] != case["profile"] or memory["status"] not in ("active", "pending"):
            continue
        values = {**memory, "keywords": " ".join(sorted(memory["keywords"]))}
        text = " ".join(values[field] for field in plan["fields"]).lower()
        if all(word.lower() in text for word in plan["include"]) and not any(word.lower() in text for word in plan["exclude"]):
            matches.add(memory["memory_uuid"])
    return matches


def validate_samples(samples):
    # What：检查合成标注、原文证据、样本划分与受控故障预期的自洽性。
    # Why：错误标签会把搜索缺陷误判为通过，必须在模型调用前阻断。
    memories = {(memory["profile"], memory["memory_uuid"]): memory for memory in samples["memories"]}
    assert len(memories) == len(samples["memories"]), "同 profile 的记忆身份重复"
    cases = {case["id"]: case for case in samples["cases"]}
    assert len(cases) == len(samples["cases"]), "用例 ID 重复"
    positives = {name: set() for name in ("tune", "holdout", "contract")}
    for case in samples["cases"]:
        actual = hard_matches(samples, case)
        expected = set(case.get("expected_hard_match_memory_uuids", case["expected_memory_uuids"]))
        assert actual == expected, f"硬条件标注错误: {case['id']}"
        assert set(case["expected_memory_uuids"]) <= actual
        positives[case["split"]].update((case["profile"], uuid) for uuid in case["expected_memory_uuids"])
        plan = normalize_request(case["request"]["params"])
        if "expected_include_text" in case:
            assert plan["include_text"] == case["expected_include_text"]
        for evidence in case.get("evidence", []) + case.get("exclusion_evidence", []):
            memory = memories[(case["profile"], evidence["memory_uuid"])]
            value = memory[evidence["field"]]
            text = " ".join(value) if isinstance(value, list) else value
            assert evidence["text"] in text, f"证据不在原文中: {case['id']}"
            if evidence.get("section"):
                assert evidence["section"] in text[:text.index(evidence["text"])], case["id"]
        if "expected_readback_summary" in case:
            for uuid in case["expected_memory_uuids"]:
                assert memories[(case["profile"], uuid)]["summary"] == case["expected_readback_summary"]
    assert positives["tune"].isdisjoint(positives["holdout"]), "调参与留出正例身份重叠"
    for case in samples["invalid_request_cases"]:
        try:
            normalize_request(case["params"])
        except ValueError:
            continue
        raise AssertionError(f"非法输入被目标契约接受: {case['id']}")
    variants = 0
    for group in samples["pipeline_cases"]:
        base = cases[group["base_case_id"]]
        for variant in group.get("variants", [group]):
            assert set(variant["expected_memory_uuids"]) <= hard_matches(samples, base)
            assert variant["expected_outcome"] in ("success", "degraded")
            if "expected_order" in variant:
                assert set(variant["expected_order"]) == set(variant["expected_memory_uuids"])
            variants += len(variant.get("conditions", [None]))
    long_memory = memories[(samples["profile"], "00000000-0000-4000-8000-000000000012")]
    assert len(long_memory["content"]) > 60000 and long_memory["content"].rindex("纸舟终点") > 60000
    assert all(marker not in memory["content"] for memory in memories.values() for marker in samples["content_expansions"])
    return {"memory_identities": len(memories), "cases": len(cases),
            "splits": dict(Counter(case["split"] for case in cases.values())),
            "invalid_requests": len(samples["invalid_request_cases"]),
            "index_scenarios": len(samples["index_cases"]), "pipeline_conditions": variants}


class ModelClient:
    def __init__(self, config):
        self.config, self.calls = config, []
        self.last_call, self.stopped = 0, False

    def post(self, section, body):
        # What：请求真实模型并记录状态、耗时、usage 与限流头。
        # Why：将评测成本与结果一并保留，缺失的计费数据不能猜成零。
        if self.stopped or len(self.calls) >= 64:
            raise RuntimeError("P0 请求预算已耗尽或上游已限流")
        time.sleep(max(0, 1 - (time.monotonic() - self.last_call)))
        self.last_call = time.monotonic()
        started = time.time() * 1000
        clock = time.monotonic()
        settings = self.config[section]
        row = {"model_type": section, "started_epoch_ms": started, "model": body.get("model"),
               "input_count": len(body.get("documents", [])) if section == "rerank" else 1}
        try:
            api = settings[section + "_api"].rstrip("/")
            kind = settings.get(section + "_api_type", section).strip("/")
            endpoint = api if api.rsplit("/", 1)[-1] == kind else api + "/" + kind
            response = requests.post(endpoint, json=body, timeout=(10, 45),
                headers={"Authorization": "Bearer " + settings[section + "_key"].strip()})
            status, data = response.status_code, response.json()
            row["rate_limit_headers"] = {key: value for key, value in response.headers.items()
                if any(word in key.lower() for word in ("ratelimit", "rate-limit", "retry-after"))}
            if status == 429:
                self.stopped = True
            row["usage"], row["meta"] = data.get("usage"), data.get("meta")
        except (requests.RequestException, ValueError) as error:
            status, data = 502, {"error": type(error).__name__}
            row["error"] = type(error).__name__
        row.update({"http_status": status, "elapsed_ms": (time.monotonic() - clock) * 1000})
        self.calls.append(row)
        return data if status == 200 else None


def source_sql(function_name):
    source = (ROOT / "src/tools/search_memory.rs").read_text(encoding="utf-8")
    section = source.split("async fn " + function_name + "(", 1)[1].split("\nasync fn ", 1)[0]
    sql = re.search(r'r#"(.*?)"#', section, re.S).group(1)
    positions = [int(value) - 1 for value in re.findall(r"\$(\d+)", sql)]
    return re.sub(r"\$\d+", "%s", sql.replace("%", "%%")), positions


def isolated_connection(url, memories, dimension):
    # What：建立连接私有的合成搜索投影和空向量表。
    # Why：未限定 schema 的旧 SQL 必须只访问这些临时表，避免触碰真实语料。
    import psycopg
    connection = psycopg.connect(url, autocommit=True, connect_timeout=10)
    try:
        connection.execute("CREATE TEMP TABLE memory_search_index (memory_uuid uuid PRIMARY KEY, status text NOT NULL, "
            "title_text text NOT NULL, summary_text text NOT NULL, keywords_text text NOT NULL, "
            "content_text text NOT NULL, recall_when_text text NOT NULL)")
        assert type(dimension) is int and dimension > 0
        connection.execute(f"CREATE TEMP TABLE memory_embeddings (memory_uuid uuid PRIMARY KEY, embedding vector({dimension}), "
            "embedding_model text NOT NULL, embedding_dimension int NOT NULL)")
        with connection.cursor() as cursor:
            cursor.executemany("INSERT INTO pg_temp.memory_search_index VALUES (%s::uuid,%s,%s,%s,%s,%s,%s)",
                [(memory["memory_uuid"], memory["status"], memory["title"], memory["summary"],
                  " ".join(sorted(memory["keywords"])), memory["content"], memory["recall_when"]) for memory in memories])
        safe = connection.execute("SELECT to_regclass('memory_search_index') = to_regclass('pg_temp.memory_search_index') "
            "AND to_regclass('memory_embeddings') = to_regclass('pg_temp.memory_embeddings')").fetchone()[0]
        assert safe, "旧 SQL 必须只访问连接私有临时表"
        connection.execute("SET default_transaction_read_only = on")
        return connection
    except Exception:
        connection.close()
        raise


def execute_candidates(connection, query, params):
    sql, positions = query
    rows = connection.execute(sql, [params[index] for index in positions]).fetchall()
    keys = ("memory_uuid", "title_norm", "status", "summary", "content_preview", "matched_fields", "score")
    return [dict(zip(keys, row)) for row in rows]


def legacy_search(connection, case, client, queries):
    # What：用原始 SQL 和真实模型复现旧版高级搜索的阶段顺序。
    # Why：保留旧版零命中保底、单候选跳过重排及部分响应追加行为，避免提前修复基线。
    plan = normalize_request(case["request"]["params"])
    flags = [field in plan["fields"] for field in FIELDS]
    params = ["", *flags, plan["include"], plan["exclude"], [], plan["limit"], False]
    rows = execute_candidates(connection, queries[0], params)
    diagnostics = {"literal_memory_uuids": [row["memory_uuid"] for row in rows]}
    strategy = {"embedding_fallback": False, "rerank": False}
    database_search_calls = 1
    if plan["include_text"] and not rows:
        config = client.config["embeddings"]
        data = client.post("embeddings", {"model": config["embeddings_model"], "input": plan["include_text"]})
        if data is not None:
            vector = data["data"][0]["embedding"]
            if len(vector) != config["embeddings_dimension"] or not all(math.isfinite(value) for value in vector):
                raise ValueError("查询向量维度或数值无效")
            params = [json.dumps(vector), *flags, plan["include"], plan["exclude"], [],
                      config["embeddings_model"], config["embeddings_dimension"], plan["limit"],
                      config.get("embeddings_fallback_max_distance", .65)]
            rows = execute_candidates(connection, queries[1], params)
            strategy["embedding_fallback"] = True
            database_search_calls += 1
    if plan["include_text"] and len(rows) > 1 and client.config["rerank"]["enabled"]:
        documents = [row["title_norm"] + "\n" + row["summary"] for row in rows]
        diagnostics["rerank_documents"] = [{"memory_uuid": row["memory_uuid"], "text": text}
            for row, text in zip(rows, documents)]
        data = client.post("rerank", {"model": client.config["rerank"]["rerank_model"],
            "query": plan["include_text"], "documents": documents, "top_n": len(rows)})
        ranked = data.get("results", []) if data is not None else []
        diagnostics["rerank_results"] = ranked
        if ranked and all(type(item["index"]) is int and 0 <= item["index"] < len(rows) for item in ranked):
            remaining, rows = list(rows), []
            for item in ranked:
                index, score = item["index"], item.get("relevance_score", item.get("score"))
                candidate, remaining[index] = remaining[index], None
                if candidate is not None and score >= .1:
                    candidate["score"] = score
                    rows.append(candidate)
            rows.extend(row for row in remaining if row is not None)
            strategy["rerank"] = True
    return {"response": {"profile": case["profile"], "data": {"results": rows, "strategy": strategy}},
            "include_text": plan["include_text"], "error": None, "database_search_calls": database_search_calls,
            "diagnostics": diagnostics}


def capture_baseline(samples, config, connection_profile):
    connections, observed = {}, {}
    client = ModelClient(config)
    queries = [source_sql(name) for name in ("search_literal_candidates", "search_embedding_fallback")]
    try:
        for profile in sorted({memory["profile"] for memory in samples["memories"]}):
            memories = [memory for memory in samples["memories"] if memory["profile"] == profile]
            connections[profile] = isolated_connection(config["database"][connection_profile], memories,
                config["embeddings"]["embeddings_dimension"])
        for case in samples["cases"]:
            started, first_call = time.monotonic(), len(client.calls)
            try:
                result = legacy_search(connections[case["profile"]], case, client, queries)
            except Exception as error:
                result = {"response": None, "error": type(error).__name__}
            result.update({"elapsed_ms": (time.monotonic() - started) * 1000, "model_calls": client.calls[first_call:]})
            observed[case["id"]] = result
            print(f"已测 {case['id']}: {result['error'] or '完成'}", flush=True)
    finally:
        for connection in connections.values():
            connection.close()
    return observed, client.calls


def score_case(samples, case, observed):
    rows = (observed.get("response") or {}).get("data", {}).get("results", [])
    actual = [row["memory_uuid"] for row in rows]
    expected = set(case["expected_memory_uuids"])
    returned = set(actual)
    allowed = hard_matches(samples, case)
    plan = normalize_request(case["request"]["params"])
    sources = {memory["memory_uuid"]: memory for memory in samples["memories"] if memory["profile"] == case["profile"]}
    checks = {"result_set": returned == expected, "no_duplicates": len(actual) == len(returned),
              "hard_filters": returned <= allowed, "limit": len(actual) <= plan["limit"],
              "profile": (observed.get("response") or {}).get("profile") == case["profile"],
              "include_text": observed.get("include_text") == plan["include_text"],
              "execution": observed.get("error") is None and observed.get("response") is not None
                  and all(call["http_status"] == 200 for call in observed["model_calls"])}
    checks["source_identity"] = all(row["memory_uuid"] in sources and
        (row.get("title_norm"), row.get("summary"), row.get("status")) ==
        (sources[row["memory_uuid"]]["title"], sources[row["memory_uuid"]]["summary"], sources[row["memory_uuid"]]["status"])
        for row in rows)
    if "expected_order" in case:
        checks["order"] = actual == case["expected_order"]
    if "expected_model_calls" in case:
        counts = Counter(row["model_type"] for row in observed["model_calls"])
        checks["model_calls"] = all(counts[kind] == count for kind, count in case["expected_model_calls"].items())
    if "expected_readback_summary" in case:
        checks["readback_summary"] = all(row["summary"] == case["expected_readback_summary"] for row in rows) and returned == expected
    return {"id": case["id"], "split": case["split"], "expected": sorted(expected), "actual": actual,
            "tp": len(returned & expected), "fp": len(returned - expected), "fn": len(expected - returned),
            "hard_filter_violations": len(returned - allowed), "duplicates": len(actual) - len(returned),
            "checks": checks, "passed": all(checks.values()), "observed": observed}


def summarize(rows):
    latencies = sorted(row["observed"]["elapsed_ms"] for row in rows)
    tp, fp, fn = (sum(row[key] for row in rows) for key in ("tp", "fp", "fn"))
    empty = [row for row in rows if not row["expected"]]
    return {"cases": len(rows), "passed": sum(row["passed"] for row in rows),
            "precision": tp / (tp + fp) if tp + fp else None, "recall": tp / (tp + fn) if tp + fn else None,
            "expected_empty_cases": len(empty), "correct_empty_cases": sum(row["passed"] for row in empty),
            "hard_filter_violations": sum(row["hard_filter_violations"] for row in rows),
            "duplicates": sum(row["duplicates"] for row in rows),
            "latency_ms": {"p50": latencies[math.ceil(len(latencies) * .5) - 1],
                           "p95": latencies[math.ceil(len(latencies) * .95) - 1], "max": max(latencies)}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--live", action="store_true", help="连接数据库 TEMP 表并调用配置中的真实模型")
    parser.add_argument("--connection-profile", default="codex", help="仅借用该连接的 TEMP 权限，不更改其持久数据")
    parser.add_argument("--report", type=Path, default=ROOT / "docs/embedding_p0_baseline.json")
    args = parser.parse_args()
    samples_path = ROOT / "docs/embedding_search_samples.json"
    samples = load_samples(samples_path)
    validation = validate_samples(samples)
    print(json.dumps({"fixture_validation": validation}, ensure_ascii=False, indent=2), flush=True)
    if not args.live:
        return
    config = tomllib.loads(Path(os.environ.get("MEM012_CONFIG", ROOT / "config.toml")).read_text(encoding="utf-8"))
    observed, calls = capture_baseline(samples, config, args.connection_profile)
    rows = [score_case(samples, case, observed[case["id"]]) for case in samples["cases"]]
    invalid = {"checked": len(samples["invalid_request_cases"]), "kind": "target_contract_oracle_only",
               "note": "不代表实际 Rust 参数解析器的全量行为；公开 query 拒绝另有源码路径证据。"}
    tracked = ["docs/embedding_search_samples.json", "docs/EMBEDDING_P0_CONTRACT.md", "src/tools/search_memory.rs",
               "src/tools/mod.rs", "scripts/embedding_p0.py", "src/provider/embedding.rs", "src/provider/rerank.rs"]
    report = {"recorded_at": datetime.now(timezone.utc).isoformat(), "kind": "python_reference_with_actual_legacy_sql",
              "isolation": "connection-private TEMP tables; empty vector table; no persistent writes",
              "source_hashes": {path: hashlib.sha256((ROOT / path).read_bytes()).hexdigest() for path in tracked},
              "git_commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
              "git_status": subprocess.check_output(["git", "status", "--short"], cwd=ROOT, text=True).splitlines(),
              "models": {"embedding": config["embeddings"]["embeddings_model"], "dimension": config["embeddings"]["embeddings_dimension"],
                         "rerank": config["rerank"]["rerank_model"], "rerank_enabled": config["rerank"]["enabled"]},
              "command": ["python3", "scripts/embedding_p0.py", "--live"],
              "fixture_validation": validation, "summary": summarize(rows),
              "splits": {split: summarize([row for row in rows if row["split"] == split]) for split in ("tune", "holdout", "contract")},
              "invalid_requests": invalid, "model_calls": calls, "cases": rows,
              "limitations": ["合成语料与小样本单次测量；不评价生产容量或 ANN 索引质量。",
                              "耗时包含评测保护所设的模型请求起始间隔，不能作为 Rust 程序性能结论。",
                              "故障/证据与分块场景当前仅固定可验证预期，行为实现留在 P1–P3。",
                              "readback_summary 校验工具返回的摘要与源标注一致，未调用线上 read_memory。",
                              "Python 参考流程执行原始 SQL，未执行完整 Rust CLI/provider；不复刻 Rust 重试/f32/连接池开销。",
                              "参数非法由目标契约 oracle 校验，现有 Rust 单元测试和源码路径证据另行记录。"]}
    args.report.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"summary": report["summary"], "report": str(args.report)}, ensure_ascii=False, indent=2))


if __name__ == "__main__":
    main()
