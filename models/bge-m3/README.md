# BGE-M3 分词文件

- 模型：`BAAI/bge-m3`
- 固定 revision：`e44369c5623cc146f016da906583db4ee0e3488d`
- 来源：https://modelscope.cn/models/BAAI/bge-m3/resolve/e44369c5623cc146f016da906583db4ee0e3488d/tokenizer.json
- 文件大小：17,098,108 bytes
- SHA256：`21106b6d7dab2952c1d496fb21d5dc9db75c28ed361a05f5020bbba27810dd08`

Rust 使用 `load_bge_m3_tokenizer` 从本地文件加载并校验，不自动联网下载。返回的 tokenizer 已关闭截断和填充；调用 `encode(完整输入, true)` 将特殊 token 一起计数。完整输入包括正文及实际拼接的标题、摘要、关键词和标题路径；BGE-M3 窗口为 8192 token。

冻结来源及计数示例见 `docs/embedding_p0_model_limits.json`。本目录只包含分词数据，不包含模型权重。
