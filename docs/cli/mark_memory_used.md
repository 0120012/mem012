# Mark Memory Used

## 1. 定位

`mark_memory_used` 用于给一条已经“实际采用”的记忆写 usage 埋点。

它只负责更新 `memory_usage`，不负责搜索、读取、更新、删除或审核。

```text
mark_memory_used = 记录一次实际采用
search_memory    = 找候选 memory_uuid
read_memory      = 读取目标记忆完整内容
read_memory_hash = 更新前读取字段 hash
update_memory_*  = 带 expected_*_hash 写入变更
delete_memory    = 删除前仍依赖明确 memory_uuid
```

`mark_memory_used` 不写入 `memory_units`、`memory_keywords`、`memory_changes`，不刷新 embedding，也不标记 graph dirty。

## 2. 何时调用

只在记忆已经进入实际输出、决策或自动流程，并且确实被采用时调用。

不调用的情况：

- 只是 search 命中
- 只是 read / preview
- 只是展示候选列表
- 只是为了检查 hash

规则：

- 一次实际采用记一次。
- 调用方不能把“看见这条记忆”当成“已经使用这条记忆”。
- `memory_uuid` 必须对应一条真实存在的记忆。
- `memory_units.status = trashed` 时拒绝更新。

## 3. 运行方式

```bash
mem012 --profile riko --args '<json_object>'
```

## 4. 请求外壳

```json
{
  "tool": "mark_memory_used",
  "params": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00"
  }
}
```

顶层规则：

- 只允许 `tool` 和 `params`。
- `tool` 必须是字符串。
- `params` 必须是 object。
- `tool` 只能是 `mark_memory_used`。

参数规则：

- 只允许 `memory_uuid`。
- `memory_uuid` 必填且不能为空。
- 禁止传 `profile`、`category`、`title`、`content`、`summary`、`keywords`。

## 5. 成功响应

```json
{
  "state": "success",
  "tool": "mark_memory_used",
  "data": {
    "memory_uuid": "8b31f4b0-2f87-4f72-bdb6-7a8c2b65aa00",
    "use_count": 3,
    "last_used_at": "2026-05-24T10:15:00Z"
  },
  "error": null,
  "profile": "riko"
}
```

规则：

- `memory_usage` 首次缺行时，插入一行再写入。
- 后续调用只做 `use_count + 1`。
- `last_used_at` 以当前时间覆盖。
- 不修改 `memory_units.updated_at`。
- 不刷新 embedding。
- 不标记 graph dirty。

## 6. 失败场景

```text
memory_uuid 为空        = 拒绝
memory_uuid 不存在      = 拒绝
memory 状态为 trashed   = 拒绝
请求包含未知字段        = 拒绝
```

## 7. 非目标

- 搜索记忆
- 读取记忆
- 更新记忆
- 删除记忆
- approve / reject
- 生成或刷新 embedding
- 计算 hash
