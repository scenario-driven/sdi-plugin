use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

pub type Timestamp = DateTime<Utc>;

pub fn now() -> Timestamp {
    Utc::now()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdKind {
    Project,
    Plan,
    Requirement,
    Decision,
    Scenario,
    Round,
    Task,
    Knowledge,
    Event,
    DisruptionReview,
    Comment,
    Question,
    Activity,
    Run,
    TaskRelation,
    UsageRecord,
}

impl IdKind {
    pub fn prefix(self) -> &'static str {
        match self {
            IdKind::Project => "PROJ",
            IdKind::Plan => "PLAN",
            IdKind::Requirement => "REQ",
            IdKind::Decision => "DEC",
            IdKind::Scenario => "SCN",
            IdKind::Round => "ROUND",
            IdKind::Task => "TASK",
            IdKind::Knowledge => "KN",
            IdKind::Event => "EVT",
            IdKind::DisruptionReview => "DRV",
            IdKind::Comment => "CMT",
            IdKind::Question => "QST",
            IdKind::Activity => "ACT",
            IdKind::Run => "RUN",
            IdKind::TaskRelation => "TREL",
            IdKind::UsageRecord => "USG",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id(pub String);

impl Id {
    pub fn new(kind: IdKind) -> Self {
        Id(format!("{}-{}", kind.prefix(), ulid::Ulid::new()))
    }

    pub fn project_from_slug(slug: &str) -> Self {
        Id(format!("PROJ-{}", slug::slugify(slug)))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for Id {
    fn from(s: String) -> Self {
        Id(s)
    }
}

impl From<&str> for Id {
    fn from(s: &str) -> Self {
        Id(s.to_string())
    }
}

pub fn new_ulid_id(kind: IdKind) -> Id {
    Id::new(kind)
}
