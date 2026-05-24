# Tool Docs

当前 CLI 工具合同已收口到：

[docs/cli/cli_memory.md](cli/cli_memory.md)

当前 Rust CLI 只保留：

```text
create_memory
delete_memory
read_memory
read_memory_hash
update_memory_replace
update_memory_patch_content
update_memory_append
update_memory_add_keywords
update_memory_remove_keywords
```

旧的 lookup、recall、search、patch、relation、graph 等 CLI 工具已经不属于当前 CLI 调用面；`search_memory` 仍是文档设计，尚未接入 Rust CLI。

`mark_memory_used` 也是文档设计，尚未接入 Rust CLI，见 [docs/cli/mark_memory_used.md](cli/mark_memory_used.md)。
