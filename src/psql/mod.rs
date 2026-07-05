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
    ApprovedEmbedding, approve_change, delete_expired_trash, delete_trash, get_change, get_trash,
    list_changes, list_trash, reject_change, restore_trash,
};
pub use graph_meta::{get_memory_graph_status, mark_memory_graph_dirty};
pub use init_db::init_db;
pub(crate) use init_db::init_profile_memory_tables;
pub use memories::{
    MemoryUpdateInput, list_memories, list_memory_category_keywords, update_memory,
};
pub(crate) use profile::{
    apply_profile_admin_setup_sql, cleanup_profile_admin_resources,
    ensure_profile_admin_resources_absent, initialize_profile_database_schema,
};
pub(crate) use relations::memory_state;
pub use relations::{
    RelationCreate, RelationUpdate, add_memory_relation, delete_memory_relation,
    list_memory_graph_overview, list_memory_neighbors, suggest_memory_relations,
    update_memory_relation,
};
