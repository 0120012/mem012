# Rust 记忆系统重写计划

## 目标

重写目标不是复刻 mem_012 旧版 URI 图，而是做一套更适合长期 Agent 记忆的后端：

```text
profile 私库 + mem_share 共享库 + category 分类 + Memory Unit + 关键词主导搜索 + 关系图 + 二次确认
```

## 已确定结论

- `profile` 只做私有数据库隔离；启动时从 TOML 选定，不进入搜索参数。
- `delete_memory` 先软删除；用户确认 delete 后立即硬删除。
- 运行时同时允许访问当前 profile 私库和 `mem_share` 共享库。
- `category` 是记忆的大类，例如 `core / meta / trace / project / book`；`share` 是共享库专属 category；不需要提前写入 TOML 白名单。
- URI 不再作为核心寻址方式，也不再有 `domain://path`。
- `kind` 不做配置白名单，不做核心寻址维度；如果需要分类，交给 category 或关键词处理。
- `disclosure` 的思想保留，字段改成 `recall_when`。
- 图继续存在，但图表达记忆之间的关系，不表达路径树。
- PostgreSQL-only；不再保留 SQLite 分支。
- MCP、HTTP、CLI 共享同一套核心服务。

## 账户与共享库边界

`mem_012/docs/PSQL_Account.md` 定义的是“一账号一私库 + 一个共享库”：

```text
profile riko -> mem_riko
shared -> mem_share
```

规则：

- 每个 profile 私库和 `mem_share` 使用同一套表结构。
- 后端运行时持有两个连接：当前 profile 私库连接、`mem_share` 连接。
- 普通写入默认进入当前 profile 私库；写入 `mem_share` 必须走明确入口。
- 搜索默认可以合并当前 profile 私库和 `mem_share`，但返回结果必须带库来源。
- 目标为 `mem_share` 时，工作态写入和二次确认记录都只作用于 `mem_share`。
- `mem_share` 只允许 `category = share`；profile 私库禁止使用 `category = share`。
- 所有读接口返回 `db_scope = profile | share`；`db_scope = profile` 时同时返回当前 profile 名。
- `memory_relations` 只在同一个数据库内建边，不做跨数据库外键。
- AGE 图也按数据库分别维护：私库一个 graph，`mem_share` 一个 graph。
- 如果以后需要跨库关系，另建引用层；第一版不把跨库关系塞进 `memory_relations`。

## 一、数据库结构

数据库分三层：

```text
源数据表：当前工作态，Agent 和用户都直接读取
派生索引：只从当前工作态生成，可重建
待确认变更表：只保存当前未二次确认的回滚凭证
```

### memory_units

记忆本体。每一条都是当前工作态；是否能进入正式召回和图谱由 `status` 决定。

```text
uuid uuid primary key
category text not null
title_norm text not null
content text not null
summary text
status text not null
recall_when text
trashed_at timestamptz
created_at timestamptz not null
updated_at timestamptz not null
```

核心约束：

```text
category 非空，并符合 slug 规则
title_norm 非空，不能为空字符串，并且等于 normalize_title(title_norm)
profile 私库禁止 category = share
mem_share 只允许 category = share
status 由后端状态机控制
pending / active memory 在同一数据库内不允许 category + title_norm 重复
```

固定 status：

```text
pending
active
trashed
```

字段职责：

- `category`：记忆的大类，不是 profile 隔离边界。
- `title_norm`：唯一标题字段，后端调用数据库 `normalize_title` 后写入；同时用于展示、搜索、embedding、唯一约束和同名判断。
- `content`：完整正文。
- `summary`：可选的语义压缩文本；缺失时不能阻塞记忆写入。
- `recall_when`：什么时候应该召回。
- `status`：后端内部状态，不放 TOML；固定为 `pending`、`active`、`trashed`。
- `trashed_at`：进入回收站的时间；只有 `status = trashed` 时有值。
- 同名判断以 `title_norm` 为准；后端可调用数据库函数提前校验，但数据库唯一约束兜底。
- `pending` 不进入正式召回和 AGE 图谱，只能通过待确认列表或 uuid 精确读取。
- `trashed` 不参与同名唯一性判断；用户确认 delete 后硬删除并释放相关派生数据。

### memory_embeddings

语义召回派生表。embedding 不属于记忆本体，可以重建，也可以在未来换维度时独立迁移。

```text
memory_uuid uuid primary key references memory_units(uuid) on delete cascade
embedding vector(1024) not null
embedding_model text not null
embedding_dimension int not null
embedded_at timestamptz not null
```

约束：

```text
embedding_dimension = 1024
```

规则：

- `embedding` 由 title_norm + summary + content + keywords 生成，只服务语义召回。
- `embedding_model` 和 `embedded_at` 给人类和运维查看；Agent 搜索时无需关注。
- create/update 写入工作态后，后端应生成或刷新 embedding；生成失败不回滚工作态，但该 memory 暂不参与语义召回。
- 写入 `memory_embeddings` 失败时，保留 `memory_units` 和 `memory_changes`，由后续重建任务补齐派生索引。
- active memory 有 embedding 时参与语义召回；历史导入、升级异常或生成失败导致缺失时，不参与语义召回。
- 第一版固定 1024 维；远程 embedding 必须请求 1024 维。
- 本地 fallback 必须选择 1024 维模型；如果本机资源不足，可以作为非常驻重建任务运行。
- 只有 title_norm / summary / content / keywords 变化时才重算 embedding；usage 变更不重算。
- 如需关系候选，可以临时生成 embedding，但不得把临时结果写入 `memory_embeddings`。
- 未来更换维度时，新建下一代表，回填完成并建好 HNSW 后再切换查询。
- `memory_changes` 不记录 `memory_embeddings`；embedding 是派生索引，由写入流程或重建任务生成。

维度升级流程：

```text
1. 创建 memory_embeddings_next，使用新的 vector(N)
2. 用新模型回填 active memory
3. 为 memory_embeddings_next 建 HNSW cosine index
4. 切换查询配置 active_embedding_table = memory_embeddings_next
5. 验证召回结果和性能
6. 旧 memory_embeddings 改名备份或删除
7. memory_embeddings_next 改名为 memory_embeddings
```

### memory_keywords

搜索主力。负责专名、项目名、术语、别名、重要触发词。

```text
uuid uuid primary key
memory_uuid uuid not null references memory_units(uuid) on delete cascade
keyword_norm text not null
weight int
created_at timestamptz not null
```

约束：

```text
unique(memory_uuid, keyword_norm)
weight is null or weight between 0 and 100
```

规则：

- `keyword_norm` 存规范化后的关键词。
- `weight` 可空；为空表示后端使用默认权重。
- 写入前和查询前都必须 normalize。

### memory_search_index

`search_memory` 专用派生搜索投影表。它不是事实来源，只从当前工作态的 `memory_units` 和 `memory_keywords` 重建。

```text
memory_uuid uuid primary key references memory_units(uuid) on delete cascade
status text not null
title_text text not null default ''
summary_text text not null default ''
keywords_text text not null default ''
content_text text not null default ''
recall_when_text text not null default ''
all_text text not null default ''
indexed_at timestamptz not null
```

字段来源：

```text
status           = memory_units.status
title_text       = memory_units.title_norm
summary_text     = memory_units.summary
keywords_text    = memory_keywords.keyword_norm 聚合
content_text     = memory_units.content
recall_when_text = memory_units.recall_when
all_text         = title / summary / keywords / content / recall_when 拼接
```

规则：

- `memory_search_index` 只能由内部刷新函数写入，Agent 和工具不能直接编辑。
- `memory_search_index` 保存当前工作态，不保存 `memory_changes.before_state` 或历史状态。
- `pending` 和 `active` 都可以被 `search_memory` 搜到。
- `filters = []` 时查询 `all_text`，字段范围是 title / summary / keywords / content / recall_when。
- `filters` 非空时只查询对应字段文本。
- `terms.all`、`terms.none`、`terms.any` 都必须在同一字段范围内判断。
- `matched_fields` 必须由实际命中的字段文本二次判断生成，不能把 `all_text` 作为返回字段。
- `preview` 只用于 content 命中时的短上下文，最长 120 字。
- 搜索时永远排除 `status = trashed`；表内可以保留 trashed 行，便于 reject/restore 后重建。
- `category` 当前不进入 `all_text`，也不作为搜索硬过滤开放。

刷新规则：

```text
create_memory 写入 pending 工作态后刷新
update_memory_* 写回 memory_units / memory_keywords 后刷新
delete_memory 标记 trashed 后刷新
restore 写回 active 后刷新
approve create 把 pending 改 active 后刷新
approve delete 硬删除 memory_units，依靠外键级联清理
approve update / restore 不改变工作态，不需要刷新
reject create 硬删除 memory_units，依靠外键级联清理
reject update / delete / restore 恢复 before_state 后刷新
```

### normalize 规则

第一版只做保守规范化，不做拼音、同义词、翻译或中文分词。

```text
category: trim + lower，必须符合 slug
title_norm: 后端调用数据库 normalize_title 后写入，数据库 check 约束校验
keyword_norm: 后端 normalize_keyword 后写入，查询时使用同一规则
query: 后端 normalize_query 后分发到 keyword / trigram / embedding
```

规则：

- `normalize_title(text)` 由 migration 创建，是 title_norm 规范化的唯一权威。
- `normalize_title` 至少执行 trim、lower、连续空白折叠；结果不能为空，不得清空中文、数字或下划线。
- 后端不得维护独立 title normalize 语义；create、update 和冲突检查都必须调用数据库函数得到 `title_norm`。
- `normalize_keyword` 至少执行 trim、lower、连续空白折叠；空字符串非法。
- 后端预检只复用数据库函数结果；数据库 check 和唯一约束仍是最终兜底。

### memory_usage

轻量使用统计。给人类看哪些记忆真的被用过，也可作为后续排序信号。

```text
memory_uuid uuid primary key references memory_units(uuid) on delete cascade
use_count int not null default 0
last_used_at timestamptz
```

规则：

- 只在记忆被实际采用时更新，不在普通搜索命中时更新。
- 更新 usage 不修改 `memory_units.updated_at`。

### memory_changes

待确认变更表。Agent 写入会先落到当前工作态；本表只记录用户二次确认和撤销所需的回滚状态。

```text
uuid uuid primary key
memory_uuid uuid not null
action text not null
before_state jsonb
after_state jsonb
created_at timestamptz not null
updated_at timestamptz not null
```

约束：

```text
action in ('create', 'update', 'delete', 'restore')
uuid = memory_uuid
unique(memory_uuid)
```

规则：

- 使用 baseline 模型：二次确认的是“最后确认状态 -> 当前工作态”。
- 表中存在记录就表示该 memory 等待用户二次确认；不再保存 accepted / rolled_back 历史。
- 同一 memory 同时最多一条 change；不建 batch 表。
- create 会同时写入 `memory_units(status = pending)`、派生索引和 `memory_changes`；`before_state` 为空，`after_state` 是待批准工作态。
- update / delete / restore 会先修改当前工作态；如果没有 open change，先保存 `before_state`；如果已有 open change，只覆盖 `after_state` 和 `updated_at`。
- 工作态写入或回滚后必须刷新 `memory_search_index`；embedding 仍按 approve 后刷新规则处理。
- state 以 JSON 保存完整工作态快照，结构固定为 memory、keywords、relations。
- 创建或更新时，重复检测以 `memory_units` 等当前工作态表和正式唯一约束为准；`memory_changes.after_state` 不承担唯一约束。
- 用户确认 create 时把 `memory_units.status` 改为 `active`，删除对应 `memory_changes`，并自动写入默认 `related_to` relations。
- 用户确认 update / restore 时只删除对应 `memory_changes`，不再把 `after_state` 二次写入工作态。
- 用户确认 delete 时删除对应 `memory_changes`，再硬删除 `memory_units`，由外键级联清理派生表。
- 用户拒绝 create 时，删除 `memory_changes`，再删除对应 `memory_units`，由外键级联清理派生表。
- 用户拒绝 update / delete / restore 时，用 `before_state` 恢复当前工作态，再删除 `memory_changes`。
- 第一版 `memory_changes.memory_uuid` 不建外键；后端事务保证 open change 指向当前工作态，拒绝 create 时也便于先删 change 再删 memory。
- update 的 title_norm、content、summary、keywords、relations 变化都会立即体现在当前工作态和 `after_state`。
- 只修改 relation 也归入 `action = update`，不单独设置 link / unlink action。
- `memory_uuid` 对所有 action 都表示当前工作态里的目标 memory；create 在同一事务内先写入 `memory_units`。
- create approve 后自动生成的默认 relations 直接进入 `memory_relations`，不再生成第二条待确认 change。
- 手动生成或修改的 relations 进入当前目标 memory 的 change。
- relation 指向的其他 memory 只是引用对象，不产生自己的 change。
- delete 会立即把 status 设为 `trashed` 并写入 `trashed_at`，同时记录 change；拒绝时从 `before_state` 恢复。
- restore 会立即把 status 设为 `active` 并清空 `trashed_at`，同时记录 change；拒绝时从 `before_state` 恢复。

state JSON 结构：

```text
memory: {uuid, category, title_norm, content, summary, status, recall_when, trashed_at}
keywords: [{keyword_norm, weight}]
relations: [{from_memory_uuid, to_memory_uuid, relation_type, weight, note}]
```

规则：

- `before_state` 是最后确认状态；create 时为 null。
- `after_state` 是当前完整工作态，不是 patch。
- `after_state.memory.title_norm` 必须已经由数据库 `normalize_title` 计算完成。
- `after_state` 不保存 `title`、`raw_title` 或 `display_title`。
- `keywords`、`relations` 都表示当前工作态的完整集合。
- `relations` 中至少一端必须等于 `memory_changes.memory_uuid`。
- `relations` 的另一端必须是同一数据库内的 active memory；`create_memory` 不建 relation，approve create 后才自动建立默认 relation。
- `memory_changes` 不记录 usage、embedding、AGE 内部数据。

### memory_relations

PostgreSQL 图表。第一版启用 Apache AGE 做图查询，但主数据仍保存在普通表；不引入 ltree。

```text
uuid uuid primary key
from_memory_uuid uuid not null references memory_units(uuid) on delete cascade
to_memory_uuid uuid not null references memory_units(uuid) on delete cascade
relation_type text not null
weight int
note text
created_at timestamptz not null
```

约束：

```text
from_memory_uuid != to_memory_uuid
unique(from_memory_uuid, to_memory_uuid, relation_type)
weight is null or weight between 0 and 100
```

规则：

- 边是有向的；查询扩展时可以同时看 outgoing 和 incoming。
- `related_to` 和 `conflicts_with` 是双向语义关系，入库时按 uuid 排序成规范方向，避免同时存在 A->B 和 B->A。
- `supersedes / depends_on / elaborates / applies_to` 保留语义方向。
- 第一版图扩展只做一跳；复杂多跳查询走 Apache AGE。
- `memory_relations` 是关系主数据；AGE 图数据由它同步生成，不直接作为正式数据源。
- `trashed` memory 的 SQL relations 保留到用户确认 delete；查询和 AGE 同步必须过滤 trashed endpoint。
- `relation_type` 由后端固定校验，不允许任意字符串写入。
- `weight` 可空；为空表示后端使用默认关系权重。
- `related_to` 是弱相关；`depends_on / supersedes / conflicts_with` 是强语义关系。
- 自动关系只能写 `related_to` 和 `supersedes`。
- `depends_on / conflicts_with / elaborates / applies_to` 只能由人类审查时修正生成。

固定关系类型：

```text
related_to
supersedes
depends_on
conflicts_with
elaborates
applies_to
```

关系类型备注：

- `related_to`：弱相关；A 和 B 有可参考关系，但不表达强因果；入库方向按 uuid 规范化。
- `supersedes`：替代；A 取代 B，方向是新记忆 -> 旧记忆。
- `depends_on`：依赖；A 的成立依赖 B，方向是依赖方 -> 被依赖方。
- `conflicts_with`：冲突；A 和 B 不能同时当真；入库方向按 uuid 规范化，查询时按双向关系处理。
- `elaborates`：展开；A 是对 B 的解释、细化，方向是细节 -> 总体。
- `applies_to`：应用；A 是 B 的落地案例，方向是案例 -> 规则/模式。

### RelationResolver

后端默认关系候选生成器。Agent 不直接决定 relation；人类只做二次确认、撤销或修正。

触发时机：

```text
create/update memory
-> begin
-> 读取当前工作态 memory / keywords / relations
-> RelationResolver 生成默认 relation 候选
-> upsert memory_changes(after_state)
-> commit
```

输入：

```text
当前工作态的 title_norm / summary / content / keywords，以及已有 memory_embeddings 行
```

候选来源：

```text
同 category
关键词强重合
embedding topK
```

自动写入限制：

- 第一版最多自动写 3 条 relation。
- 只允许自动写 `related_to` 和 `supersedes`。
- `related_to` 需要关键词强重合或 embedding 高相似。
- `supersedes` 需要同主题，并且 title_norm / summary / content 出现明确替代、更新、废弃语义。
- 不对 `trashed` memory 建 relation。
- 不跨数据库建 relation。
- relation 只能关联同一数据库内的 active memory；`trashed` memory 不参与。
- 已存在相同 `from_memory_uuid / to_memory_uuid / relation_type` 时不重复写入。
- `related_to / conflicts_with` 写入前必须先做 uuid 规范化，再检查重复。
- 所有自动 relation 只进入当前目标 memory 的 `after_state.relations`。
- 人类二次确认前可以接受、撤销，或把 relation_type 修正为 `depends_on / conflicts_with / elaborates / applies_to`。

### memory_graph_meta

AGE 派生层状态表。它不是同步队列，只记录 AGE 图是否落后于 SQL 主数据。

```text
graph_name text primary key
dirty boolean not null
updated_at timestamptz not null
```

规则：

- 每个数据库一行，私库和 `mem_share` 各自维护自己的 dirty 状态。
- `dirty = true` 表示 AGE 图可能落后于 `memory_units` / `memory_relations`。
- 写入工作态或拒绝回滚导致 `memory_units` / `memory_relations` 可见性变化时，在同一 SQL 事务内把 `dirty` 标记为 true。
- approve create 自动写入默认 relations 时必须标记 `dirty`。
- approve update/relation 只删除 `memory_changes`，不改工作态，不标记 `dirty`。
- AGE rebuild 成功后再把 `dirty` 标记为 false。
- AGE rebuild 失败不影响 SQL 主数据，也不恢复 `memory_changes`。
- 不保存变更事件，不记录历史，不做 per-memory 同步队列。

### age_graph

Apache AGE 是 PostgreSQL 内的派生图查询层，不新增业务主数据表，不作为正式数据源。

```text
graph name: memory_graph
vertex label: Memory
edge labels: RELATED_TO / DEPENDS_ON / SUPERSEDES / CONFLICTS_WITH / ELABORATES / APPLIES_TO
```

Memory vertex properties：

```text
uuid text
category text
title_norm text
status text
summary text
```

Edge properties：

```text
relation_uuid text
weight int
note text
created_at text
```

规则：

- SQL 主数据是当前工作态的 `memory_units` 和 `memory_relations`。
- AGE 内部表由 `create_graph('memory_graph')` 和 label 创建，业务代码不直接写 AGE schema。
- create/update/delete/restore 写入工作态时应更新 SQL 主表和派生索引，并在关系或可见性变化时把 `memory_graph_meta.dirty` 标记为 true。
- reject 回滚导致 memory 或 relation 变化时也必须标记 `dirty = true`。
- AGE 不进入确认事务；AGE 失败不阻塞 memory 确认。
- AGE sync worker 只在 `dirty = true` 时从 SQL 主表整图重建。
- 不建 AGE 同步队列表；需要修复时仍从 SQL 主表整图重建。
- `trashed` memory 不进入 AGE。
- relation 删除写入工作态后标记 `dirty`；撤销时如恢复 relation，也需要重新标记 `dirty`。
- 不把 AGE 内部 id 写回业务表；所有同步都靠 `uuid` 和 `relation_uuid`。
- AGE 可以整图重建：清空图后从 `memory_units` / `memory_relations` 回放。
- `dirty = true` 时复杂 AGE 查询必须拒绝或降级到 SQL 一跳关系查询。

## 二、索引设计

第一版只建必要索引。精确定位、排序、向量召回、模糊搜索分开处理。

结构索引：

```text
(category, status)
memory_units(status, updated_at)
memory_units(category, title_norm) unique where status in ('pending', 'active')
memory_embeddings(memory_uuid) primary key btree
memory_embeddings(embedding) HNSW cosine
memory_embeddings(embedded_at)
memory_search_index(memory_uuid) primary key btree
memory_search_index(status)
memory_usage(use_count)
memory_usage(last_used_at)
memory_relations(from_memory_uuid)
memory_relations(to_memory_uuid)
memory_relations(relation_type)
memory_changes(memory_uuid) unique btree
memory_changes(updated_at)
```

精确定位索引：

```text
memory_keywords(memory_uuid, keyword_norm) unique btree
memory_keywords(keyword_norm, memory_uuid) btree
```

中文分路召回：

```text
keyword exact -> memory_keywords.keyword_norm btree
keyword fuzzy -> memory_search_index.keywords_text GIN trigram
title / summary / content / recall_when fuzzy -> memory_search_index 对应字段 GIN trigram
filters = [] -> memory_search_index.all_text GIN trigram
```

语义搜索：

```text
title_norm + summary + content + keywords -> memory_embeddings.embedding
memory_embeddings.embedding -> HNSW cosine index
```

模糊搜索：

```text
memory_search_index.title_text -> GIN trigram
memory_search_index.summary_text -> GIN trigram
memory_search_index.keywords_text -> GIN trigram
memory_search_index.content_text -> GIN trigram
memory_search_index.recall_when_text -> GIN trigram
memory_search_index.all_text -> GIN trigram
```

规则：

- `search_memory` 字面召回优先查询 `memory_search_index`，避免运行时重复 join `memory_units` 和 `memory_keywords`。
- embedding 使用 cosine 距离；没有 `memory_embeddings` 行的 memory 不参与语义召回。
- 第一版不使用 PostgreSQL fulltext / tsvector；中文召回由 keyword、trigram、embedding 分路完成。

## 三、图查询规则

```text
lookup_memory(memory_uuid)
-> 读 memory_units
-> 读 keywords / usage
-> 读 memory_relations outgoing + incoming 一跳
```

```text
recall_memory(query)
-> keyword / trigram / embedding 分路召回候选
-> 对候选做 memory_relations 一跳扩展
-> relation_type + weight 只影响排序，不单独决定命中
```

图不负责分类。分类由 `category` 和 `keyword_norm` 负责。

## 四、搜索合并规则

跨库搜索由后端合并结果，Agent 不需要指定来源。

```text
recall_memory(query)
1. normalize query
2. 在目标库集合执行 keyword exact / trigram / embedding 分路召回
4. 收集候选 memory_uuid，并合并 match_sources
5. 对候选做 memory_relations 一跳扩展
6. 按 db_scope、status、category 过滤
7. 按 db_scope + memory_uuid 去重
8. 计算 score 并排序
9. 返回结果
```

规则：

- 查询在当前 profile 私库和 `mem_share` 分别执行 keyword / trigram / embedding。
- 合并结果只按 `db_scope + memory_uuid` 去重。
- profile 和 share 没有共同 identity；同名结果同时存在时，作为两条不同结果返回。
- 返回结果必须包含 `db_scope`、`profile`、`memory_uuid`、`title_norm`、`score`、`match_sources`。

状态过滤：

- `recall_memory` 默认只返回 `active`。
- `trashed` 永远不返回。
- `pending` 只能在待确认流程或 uuid 精确读取中出现。
- 所有正式召回分路和关系扩展都必须 join `memory_units` 并过滤 `status = active`。

排序优先级：

```text
handle_exact
keyword_exact
handle_trgm
keyword_trgm
title_trgm
embedding
summary_trgm
content_trgm
graph
usage
```

`match_sources` 固定取值：

```text
handle_exact
keyword_exact
handle_trgm
keyword_trgm
title_trgm
summary_trgm
content_trgm
embedding
graph
usage
```

排序规则：

- 每个候选 memory_uuid 可以有多个 `match_sources`。
- 主排序由最高优先级 source 决定。
- 其他 source 只做小幅加分。
- `usage` 只做加分，不能让未命中 memory 进入候选集。
- `graph` 只能扩展已有候选的一跳关系，不能单独从空查询产生候选。

## 五、审查与撤销流程

Agent 写入流程：

```text
begin
读取当前工作态；如果没有 open change，保存 before_state
生成 after_state 工作态
写入 memory_units / keywords / relations
刷新 memory_search_index
如果没有 change，写 memory_changes(before_state, after_state)
如果已有 change，只更新 after_state 和 updated_at
commit
```

回收站流程：

```text
delete：status = trashed，trashed_at = now()，并写入或更新 memory_changes
restore：校验没有 active 同名冲突，status = active，trashed_at = null，并写入或更新 memory_changes
approve delete：删除 memory_changes，再硬删除 memory_units，级联删除派生数据
```

确认流程：

```text
begin
锁定并读取 memory_changes
确认当前工作态仍与 after_state 一致
如果 action = delete：删除 memory_changes，再删除 memory_units
如果 action = create：memory_units.status 改为 active，刷新 memory_search_index，再删除 memory_changes
如果 action = update 或 restore：删除 memory_changes
commit
```

如果确认时发现当前工作态已和 after_state 不一致：

```text
rollback
memory_changes 保留
返回 stale_change 错误
```

拒绝应用规则：

```text
create：删除 memory_changes，再删除 memory_units，级联清理派生数据
update：用 before_state 恢复 memory_units / keywords / memory_relations，刷新 memory_search_index，再删除 memory_changes
delete：用 before_state 恢复 active 状态和派生数据，刷新 memory_search_index，再删除 memory_changes
restore：用 before_state 恢复 trashed 状态，刷新 memory_search_index，再删除 memory_changes
```

自动清理规则：

- 第一版不做自动 purge。
- 硬删除只能由用户确认 delete 触发。

撤销流程：

```text
begin
锁定并读取 memory_changes
按 action 应用拒绝规则
commit
```

## 六、PostgreSQL 扩展

第一版启用三个 PostgreSQL 扩展。搜索依赖 pgvector / pg_trgm；图查询依赖 Apache AGE。

```text
必需扩展：
pgvector：用于 memory_embeddings.embedding 的语义召回
pg_trgm：用于 keyword_norm、title_norm、summary、content 的模糊匹配
Apache AGE：用于 memory_units / memory_relations 的图查询
```

明确不使用：

```text
ltree：Agent 不按路径树搜索记忆
```

## 当前不做

- 不做 URI 兼容层。
- 不做 SQLite。
- 不做人类搜索 DSL 配置项。
- 不做自动摘要后台任务。
- 不做前端 UI 细节。
