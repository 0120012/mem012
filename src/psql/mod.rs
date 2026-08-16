mod age_graph;
mod changes;
mod graph_meta;
mod init_db;
mod memories;
mod profile;
mod relations;
pub(crate) mod search_index;

pub use age_graph::rebuild_memory_graph;
pub use changes::{
    ApprovedEmbedding, approve_change, count_changes, delete_expired_trash, delete_trash,
    get_change, get_trash, list_changes, list_trash, refresh_memory_embedding, reject_change,
    restore_trash,
};
pub use graph_meta::{get_memory_graph_status, mark_memory_graph_dirty};
pub use init_db::init_db;
pub(crate) use init_db::init_profile_memory_tables;
pub use memories::{
    MemoryUpdateInput, list_memories, list_memory_category_keywords, update_memory,
};
pub(crate) use profile::{
    apply_profile_admin_setup_sql, apply_shared_profile_admin_setup_sql,
    apply_shared_profile_database_setup_sql, cleanup_profile_admin_resources,
    cleanup_shared_profile_resources, ensure_profile_admin_resources_absent,
    ensure_shared_profile_schema, ensure_shared_profile_target, initialize_profile_database_schema,
    validate_target_database_name,
};
pub(crate) use relations::memory_state;
pub use relations::{
    RelationCreate, RelationUpdate, add_memory_relation, delete_memory_relation,
    list_memory_graph_overview, list_memory_neighbors, suggest_memory_relations,
    update_memory_relation,
};
