use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};

pub type ProjectId = Id;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    /// Short ticket prefix (e.g. "SDI"). Globally unique.
    pub key: String,
    pub name: String,
    /// URL-safe slug derived from name. Globally unique.
    pub slug: String,
    /// Anchored working directories — entry to "active project" detection.
    pub cwds: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
