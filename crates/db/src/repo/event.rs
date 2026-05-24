//! Event log — append-only stream the daemon broadcasts over SSE
//! (`GET /events`). Entries are durable so a reconnecting client can replay.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, s, s_opt, ts};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Row};
use sdi_core::error::DomainResult;
use sdi_core::ids::{new_ulid_id, now, Id, IdKind};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Event {
    pub id: Id,
    pub project_id: Option<Id>,
    pub kind: String,
    pub entity_id: Option<Id>,
    pub payload: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

fn row_to_event(row: &Row<'_>) -> rusqlite::Result<Event> {
    let payload_raw: String = row.get(4)?;
    let payload: serde_json::Value = serde_json::from_str(&payload_raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Event {
        id: Id::from(s(row, 0)?),
        project_id: s_opt(row, 1)?.map(Id::from),
        kind: s(row, 2)?,
        entity_id: s_opt(row, 3)?.map(Id::from),
        payload,
        created_at: ts(row, 5)?,
    })
}

const COLS: &str = "id, project_id, kind, entity_id, payload, created_at";

pub fn append(
    conn: &Connection,
    project_id: Option<&Id>,
    kind: &str,
    entity_id: Option<&Id>,
    payload: &serde_json::Value,
) -> DomainResult<Event> {
    let ev = Event {
        id: new_ulid_id(IdKind::Event),
        project_id: project_id.cloned(),
        kind: kind.to_string(),
        entity_id: entity_id.cloned(),
        payload: payload.clone(),
        created_at: now(),
    };
    let payload_s = serde_json::to_string(&ev.payload).unwrap_or_else(|_| "{}".into());
    conn.execute(
        &format!("INSERT INTO events({COLS}) VALUES (?1,?2,?3,?4,?5,?6)"),
        params![
            ev.id.as_str(),
            ev.project_id.as_ref().map(|i| i.as_str().to_string()),
            ev.kind,
            ev.entity_id.as_ref().map(|i| i.as_str().to_string()),
            payload_s,
            fmt_ts(ev.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(ev)
}

pub fn tail_since(
    conn: &Connection,
    project_id: Option<&Id>,
    since: Option<DateTime<Utc>>,
    limit: usize,
) -> DomainResult<Vec<Event>> {
    let mut sql = format!("SELECT {COLS} FROM events WHERE 1=1");
    let mut args: Vec<String> = Vec::new();
    if let Some(p) = project_id {
        sql.push_str(" AND project_id = ?");
        sql.push_str(&format!("{}", args.len() + 1));
        args.push(p.as_str().to_string());
    }
    if let Some(t) = since {
        sql.push_str(" AND created_at > ?");
        sql.push_str(&format!("{}", args.len() + 1));
        args.push(fmt_ts(t));
    }
    sql.push_str(&format!(" ORDER BY created_at LIMIT ?{}", args.len() + 1));
    args.push((limit as i64).to_string());
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
    let arg_refs: Vec<&dyn rusqlite::ToSql> =
        args.iter().map(|a| a as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(arg_refs.as_slice(), row_to_event)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::project as proj_repo;
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::project::Project;

    #[test]
    fn append_and_tail() {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-event-{}-{}.db",
            std::process::id(),
            ulid::Ulid::new()
        ));
        let _ = std::fs::remove_file(&tmp);
        let pool = open_pool(&tmp).unwrap();
        ensure_schema(&pool.get().unwrap()).unwrap();
        let conn = pool.get().unwrap();
        let proj = Project {
            id: Id::new(IdKind::Project),
            key: "TST".into(),
            name: "n".into(),
            slug: format!("s-{}", ulid::Ulid::new()),
            cwds: vec![],
            created_at: now(),
            updated_at: now(),
        };
        proj_repo::insert(&conn, &proj).unwrap();
        let _ = append(
            &conn,
            Some(&proj.id),
            "plan.approved",
            Some(&Id::from("PLAN-X")),
            &serde_json::json!({"plan_id": "PLAN-X"}),
        )
        .unwrap();
        let tail = tail_since(&conn, Some(&proj.id), None, 10).unwrap();
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].kind, "plan.approved");
    }
}
