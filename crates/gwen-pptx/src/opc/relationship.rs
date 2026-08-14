use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Relationship {
    pub id: String,
    pub target: String,
    pub target_mode: Option<String>,
    pub rel_type: String,
}
