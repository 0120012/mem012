use axum::Json;
use serde_json::{Value, json};

// Why：前端域列表接口在无数据阶段也应返回数组，避免页面把占位对象误判成异常结构。
pub async fn domains() -> Json<Value> {
    Json(json!([]))
}

pub async fn get_node() -> Json<Value> {
    // Why：前端详情页依赖固定的 node/children/breadcrumbs 结构，先返回空根节点避免占位阶段直接崩页。
    Json(json!({
        "node": {
            "path": "",
            "domain": "core",
            "uri": "core://",
            "name": "",
            "content": "",
            "priority": 0,
            "disclosure": "",
            "created_at": null,
            "is_virtual": true,
            "aliases": [],
            "node_uuid": null,
            "glossary_keywords": [],
            "glossary_matches": []
        },
        "children": [],
        "breadcrumbs": []
    }))
}

pub async fn put_node() -> Json<Value> {
    // Why：前端保存节点后不依赖响应正文，只要拿到稳定成功标记即可继续刷新页面。
    Json(json!({ "ok": true }))
}

pub async fn get_glossary() -> Json<Value> {
    // Why：关键词列表在占位阶段先返回空数组，避免前端把对象形状误判成异常数据。
    Json(json!([]))
}

pub async fn create_glossary() -> Json<Value> {
    // Why：前端新增关键词后会主动刷新节点数据，当前阶段只需返回稳定成功标记。
    Json(json!({ "ok": true }))
}

pub async fn delete_glossary() -> Json<Value> {
    // Why：前端删除关键词后同样会主动刷新节点数据，当前阶段只需返回稳定成功标记。
    Json(json!({ "ok": true }))
}
