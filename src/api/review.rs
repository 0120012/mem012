use axum::Json;
use serde_json::{Value, json};

pub async fn groups() -> Json<Value> {
    // Why：待审核列表在占位阶段先返回空数组，避免前端把对象形状误判成异常结构。
    Json(json!([]))
}

pub async fn group_diff() -> Json<Value> {
    // Why：审核详情页依赖固定 diff 骨架，先返回“无变更”结构，避免占位阶段渲染链断裂。
    Json(json!({
        "action": "modified",
        "has_changes": false,
        "before_meta": {
            "priority": null,
            "disclosure": null
        },
        "current_meta": {
            "priority": null,
            "disclosure": null
        },
        "path_changes": [],
        "active_paths": [],
        "glossary_changes": [],
        "before_content": "",
        "current_content": ""
    }))
}

pub async fn rollback_group() -> Json<Value> {
    // Why：前端回滚后会重新拉取审核列表，这里先返回稳定成功标记，避免占位阶段误报失败。
    Json(json!({ "success": true }))
}

pub async fn delete_group() -> Json<Value> {
    // Why：前端批准一个审核分组后会重新拉取列表，这里先返回稳定成功标记即可。
    Json(json!({ "ok": true }))
}

pub async fn clear_review() -> Json<Value> {
    // Why：前端清空全部审核项后不依赖响应正文，只需要稳定成功标记即可继续刷新界面。
    Json(json!({ "ok": true }))
}
