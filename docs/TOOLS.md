# CLI V10 Tool Reference

> 本文档是 `CLI_V10.md` 的简明工具索引。正式合同以 `CLI_V10.md` 为准。

## 1. 统一调用

```bash
llm_memory --profile DOGE --args '{"tool":"lookup_memory_by_handle","args":{"handle":"core/backend/database/profile隔离"}}'
```

规则：

- `--profile` 只选择数据库 profile，不进入工具语义。
- `args.tool` 是工具名。
- `args.args` 是工具参数对象。
- 禁止 URI，不再接受 `domain://path`。
- 写操作必须记录 version/review，前端可审查和撤销。

## 2. 读取与召回

### lookup_memory

按 `memory_uuid` 精确读取。

```json
{"tool":"lookup_memory","args":{"memory_uuid":"..."}}
```

### lookup_memory_by_handle

按 handle 精确定位。handle 必须唯一命中。

```json
{"tool":"lookup_memory_by_handle","args":{"handle":"core/backend/database/profile隔离"}}
```

### recall_memory

Agent 上下文召回。

```json
{
  "tool": "recall_memory",
  "args": {
    "query": "profile 和 category 怎么区分",
    "category": "core",
    "context_text": "正在设计 Rust 记忆系统",
    "task_mode": "design",
    "limit": 8
  }
}
```

### search_memory

人类/管理端自由组合搜索。Agent 默认不应用它做上下文召回。

```json
{"tool":"search_memory","args":{"query":"profile 隔离","category":"core","limit":20}}
```

## 3. 写入与维护

### create_memory

```json
{
  "tool": "create_memory",
  "args": {
    "category": "core",
    "title": "Profile 隔离规则",
    "content": "profile 是数据库隔离边界。",
    "summary": "profile 用于选择数据库连接。",
    "keywords": ["profile", "数据库隔离"],
    "recall_when": "当讨论 profile 或多库隔离时召回",
    "handles": ["core/backend/database/profile隔离"]
  }
}
```

### patch_memory

```json
{"tool":"patch_memory","args":{"memory_uuid":"...","old_string":"旧文本","new_string":"新文本","change_reason":"修正描述"}}
```

### update_memory_meta

```json
{"tool":"update_memory_meta","args":{"memory_uuid":"...","keywords":["profile"],"change_reason":"更新关键词"}}
```

### delete_memory

```json
{"tool":"delete_memory","args":{"memory_uuid":"...","mode":"deprecate","change_reason":"已被替代"}}
```

## 4. 图关系

### link_memory

```json
{"tool":"link_memory","args":{"from_memory_uuid":"...","to_memory_uuid":"...","relation_type":"depends_on","weight":80}}
```

### unlink_memory

```json
{"tool":"unlink_memory","args":{"relation_uuid":"..."}}
```

## 5. 审查与撤销

```json
{"tool":"review_changes","args":{"limit":20}}
```

```json
{"tool":"rollback_change","args":{"review_item_uuid":"..."}}
```

## 6. 工具清单

- `create_memory`
- `lookup_memory`
- `lookup_memory_by_handle`
- `recall_memory`
- `search_memory`
- `patch_memory`
- `update_memory_meta`
- `delete_memory`
- `link_memory`
- `unlink_memory`
- `list_categories`
- `review_changes`
- `rollback_change`
