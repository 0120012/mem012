# memory 工作态与二次确认计划

## Context

- “第一次确认”是 Agent 对自己输出的确认；“第二次确认”才是用户确认。
- 用户确认前，Agent 仍然必须能回读、查询、继续使用这条记忆。
- 现有表已经足够表达这个流程，不新增表字段。
- `memory_units` 是当前工作态；Agent 和用户都读这里。
- `memory_changes` 是否存在表示是否等待用户二次确认。
- `memory_changes.before_state` 是回滚基线；create 时为 `null`。
- `memory_changes.after_state` 是当前等待用户确认的最终态。
- 用户未确认前直接更改记忆时，只更新同一条 `memory_changes.after_state`，不新建第二条 change。

## Runtime Flow

```text
create_memory
-> transaction
-> insert memory_units
-> insert keywords / handles / relations
-> insert memory_changes(action = create, before_state = null, after_state = current_state)
-> commit
-> re-read memory_units
-> return database state
```

```text
edit memory with open change
-> transaction
-> lock memory_units + memory_changes
-> update memory_units / derived rows
-> rebuild current_state from database
-> update same memory_changes.after_state
-> commit
-> re-read memory_units
```

```text
user approve
-> delete memory_changes row
```

```text
user reject create
-> transaction
-> delete memory_changes row
-> delete memory_units row
-> cascade deletes derived rows
```

```text
user reject update/delete/restore
-> transaction
-> restore memory_units and derived rows from before_state
-> delete memory_changes row
```

## Constraints

- 不新增 `review_state`；是否待确认由 `memory_changes` 是否存在派生。
- 不新增 `revision`；第一版先依赖事务锁和回读，避免提前引入并发状态。
- `status` 只表示生命周期：`active` / `trashed`。
- `memory_units` 永远代表当前可读、可查、可继续修改的工作态。
- `memory_changes` 不保存 approved/rejected 历史；确认或回滚后删除。
- 同一个 `memory_uuid` 同时最多一条 open change，由现有 `UNIQUE(memory_uuid)` 保证。
- 每次修改 `memory_units` 后，如果存在 open change，必须同步更新 `after_state`。
- 普通 lookup/search 直接读 `memory_units`；响应里的 pending 状态由 left join `memory_changes` 派生。
- 后续若真实出现多客户端覆盖问题，再单独引入版本字段；现在不提前加。

## Checklist

- [x] 更新 `build_after_state` 语义：它表示当前完整工作态，不再表示“未生效提案”。
- [x] 把 `create_memory` 改为事务写入 `memory_units` 和 `memory_changes`。
- [x] 在 `create_memory` 事务里写入 `memory_keywords`。
- [x] 在 `create_memory` 事务里写入 `memory_handles`。
- [x] 更新重复检测：正式表存在即禁止重复，open change 只作为 pending 状态来源。
- [ ] 实现 user approve：删除 open change。
- [ ] 实现 user reject create：删除 open change 和对应 `memory_units`。
- [ ] 实现 user reject update/delete/restore：用 `before_state` 恢复工作态并删除 open change。
- [x] 更新 `docs/DB_PLAN.md`，使数据库合同与现有表工作态模型一致。
- [x] 更新 `docs/CLI_V10.md`，使 create_memory 与现有表工作态模型一致。
- [ ] 更新 `docs/CLI_V10.md`，补充 approve/reject 响应与现有表工作态模型。
- [x] 用 `cargo check -q` 验证编译。

## Later

- [ ] 需要完整审计时，再新增 `memory_events`，不要把历史塞回 `memory_changes`。
- [ ] 真实并发覆盖成为问题时，再给 `memory_units` 增加版本字段。
- [ ] `create_memory` 收尾后，再单独规划 `update_memory`。
