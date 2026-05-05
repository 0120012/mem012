use axum::Json;
use serde_json::{Value, json};

pub async fn orphans() -> Json<Value> {
    // Why：清理列表在占位阶段先返回空数组，保证前端能进入“System Clean”分支而不是报结构错误。
    Json(json!([]))
}

pub async fn orphan_detail() -> Json<Value> {
    // Why：清理详情页至少要拿到 content 字段，先返回最小骨架，避免展开卡片时进入错误分支。
    Json(json!({
        "content": "",
        "migration_target": null
    }))
}

pub async fn delete_orphan() -> Json<Value> {
    // Why：前端删除待清理记忆后会直接更新本地列表，当前阶段只需返回稳定成功标记。
    Json(json!({ "ok": true }))
}
