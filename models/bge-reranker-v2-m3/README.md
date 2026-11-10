# BGE reranker 分词文件

- 模型：`BAAI/bge-reranker-v2-m3`
- 固定 revision：`e099d4b9cdbd291b1569d416f19aaa6523570bc3`
- 来源：[ModelScope 固定分词文件](https://modelscope.cn/models/BAAI/bge-reranker-v2-m3/resolve/e099d4b9cdbd291b1569d416f19aaa6523570bc3/tokenizer.json)
- SHA256：`69564b696052886ed0ac63fa393e928384e0f8caada38c1f4864a9bfbf379c15`
- 窗口：8192 token；按 query/document pair 计入四个特殊 token。

文件来自既有 `docs/embedding_p0_model_limits.json` 冻结记录。运行时不联网下载；缺失、hash 不符或未知模型均触发 `rerank_failed` 回退。
