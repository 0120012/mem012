# Read Memory

## 1. 目标

`read_memory` 用于通过 `memory_uuid` 读取一条记忆的当前工作态内容。

核心原则：

```text
read_memory = 只读完整工作态
read_memory_hash = 只读 revision 和字段 hash
update_memory_* = 依赖 revision + hash 执行写入
```

读取范围：

```text
memory   = 记忆主体字段
keywords = 关键词列表
relations = 相关关系列表
```

`read_memory` 不写入 `memory_units`、`memory_keywords`、`memory_changes`，不刷新 embedding，也不标记 graph dirty。

## 2. 请求

```json
{
  "tool": "read_memory",
  "params": {
    "memory_uuid": "{memory_uuid}"
  }
}
```

规则：

- 只允许通过 `memory_uuid` 读取。
- `memory_uuid` 必填且不能为空。
- 禁止传 `profile`、`title`、`category`、`content`。
- `memory_units.status = trashed` 时拒绝读取。

## 3. 成功响应

```json
{
  "state": "success",
  "tool": "read_memory",
  "data": {
    "memory_uuid": "{memory_uuid}",
    "memory": {
      "uuid": "{memory_uuid}",
      "category": "core",
      "title_norm": "记忆标题",
      "content": "记忆正文",
      "summary": "记忆摘要",
      "status": "active",
      "recall_when": null,
      "trashed_at": null
    },
    "keywords": [
      {
        "keyword_norm": "关键词",
        "weight": null
      }
    ],
    "relations": []
  },
  "error": null,
  "profile": "{profile}"
}
```

规则：

- `summary` 可以是 `null`。
- `recall_when` 可以是 `null`。
- `keywords` 无关键词时返回空数组。
- `relations` 无关系时返回空数组。

## 4. 成功验证

执行后检查：

```text
1. state = success
2. tool = read_memory
3. data.memory_uuid 等于请求里的 memory_uuid
4. data.memory.uuid 等于请求里的 memory_uuid
5. data.memory.content 返回完整正文
6. data.keywords 返回当前关键词列表
7. data.relations 返回当前关系列表
```

如果需要确认后续更新版本锁，不能用 `read_memory` 结果自行生成 revision 或 hash，必须再调用 `read_memory_hash`。

## 5. 和 read_memory_hash 的关系

`read_memory` 用于查看内容，`read_memory_hash` 用于更新前拿 `revision` 和字段 hash。

更新记忆前必须执行：

```text
1. read_memory 确认当前内容
2. read_memory_hash 获取 revision 和字段 hash
3. update_memory_* 携带 expected_revision 和 expected_*_hash 执行更新
```

## 6. 失败场景

```text
memory_uuid 为空        = 拒绝
memory_uuid 不存在      = 拒绝
memory 状态为 trashed   = 拒绝
请求包含未知字段        = 拒绝
```

## 7. 非目标

- 搜索记忆
- 列出全部记忆
- 更新记忆
- 删除记忆
- approve / reject
- 生成或刷新 embedding
- 计算 `expected_revision` 或 `expected_*_hash`
