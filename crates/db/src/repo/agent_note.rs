//! AgentNote repository (M1 Blackboard). Append-only journal of inter-agent
//! communication. Retirement is non-destructive (set `retired_at`); active
//! reads use the partial index `agent_note_active_idx`.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, json, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::agent_note::{AgentNote, AgentNoteKind, AgentNoteScope};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};

const COLS: &str = "id, project_id, plan_id, round_id, scenario_id, task_id, \
                    scope_kind, kind, from_agent, to_agent, body, payload, \
                    receipt_acknowledged_at, retired_at, retired_reason, created_at";

fn row_to_note(row: &Row<'_>) -> rusqlite::Result<AgentNote> {
    Ok(AgentNote {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        plan_id: s_opt(row, 2)?.map(Id::from),
        round_id: s_opt(row, 3)?.map(Id::from),
        scenario_id: s_opt(row, 4)?.map(Id::from),
        task_id: s_opt(row, 5)?.map(Id::from),
        scope_kind: parsed::<AgentNoteScope>(row, 6)?,
        kind: parsed::<AgentNoteKind>(row, 7)?,
        from_agent: s(row, 8)?,
        to_agent: s_opt(row, 9)?,
        body: s(row, 10)?,
        payload: json::<serde_json::Value>(row, 11)?,
        receipt_acknowledged_at: ts_opt(row, 12)?,
        retired_at: ts_opt(row, 13)?,
        retired_reason: s_opt(row, 14)?,
        created_at: ts(row, 15)?,
    })
}

pub fn insert(conn: &Connection, note: &AgentNote) -> DomainResult<()> {
    AgentNote::validate_anchor(
        note.scope_kind,
        note.plan_id.as_ref(),
        note.round_id.as_ref(),
        note.scenario_id.as_ref(),
        note.task_id.as_ref(),
    )?;
    AgentNote::validate_handoff(note.kind, note.to_agent.as_deref())?;
    let payload = serde_json::to_string(&note.payload)
        .map_err(|e| DomainError::Validation(format!("encode payload: {e}")))?;
    conn.execute(
        &format!(
            "INSERT INTO agent_notes({COLS}) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)"
        ),
        params![
            note.id.as_str(),
            note.project_id.as_str(),
            note.plan_id.as_ref().map(|i| i.as_str().to_string()),
            note.round_id.as_ref().map(|i| i.as_str().to_string()),
            note.scenario_id.as_ref().map(|i| i.as_str().to_string()),
            note.task_id.as_ref().map(|i| i.as_str().to_string()),
            note.scope_kind.to_string(),
            note.kind.to_string(),
            note.from_agent,
            note.to_agent,
            note.body,
            payload,
            note.receipt_acknowledged_at.map(fmt_ts),
            note.retired_at.map(fmt_ts),
            note.retired_reason,
            fmt_ts(note.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<AgentNote> {
    conn.query_row(
        &format!("SELECT {COLS} FROM agent_notes WHERE id = ?1"),
        [id.as_str()],
        row_to_note,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

/// Active (non-retired) notes anchored to the given scope id. `kind` filters
/// optionally — pass `None` to read every active note in scope.
pub fn list_active(
    conn: &Connection,
    scope: AgentNoteScope,
    anchor_id: &Id,
    kind: Option<AgentNoteKind>,
) -> DomainResult<Vec<AgentNote>> {
    let col = match scope {
        AgentNoteScope::Plan => "plan_id",
        AgentNoteScope::Round => "round_id",
        AgentNoteScope::Scenario => "scenario_id",
        AgentNoteScope::Task => "task_id",
    };
    let (sql, args): (String, Vec<String>) = match kind {
        Some(k) => (
            format!(
                "SELECT {COLS} FROM agent_notes \
                 WHERE scope_kind = ?1 AND {col} = ?2 AND retired_at IS NULL \
                   AND kind = ?3 \
                 ORDER BY created_at"
            ),
            vec![scope.to_string(), anchor_id.as_str().to_string(), k.to_string()],
        ),
        None => (
            format!(
                "SELECT {COLS} FROM agent_notes \
                 WHERE scope_kind = ?1 AND {col} = ?2 AND retired_at IS NULL \
                 ORDER BY created_at"
            ),
            vec![scope.to_string(), anchor_id.as_str().to_string()],
        ),
    };
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
    let arg_refs: Vec<&dyn rusqlite::ToSql> = args.iter().map(|a| a as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(arg_refs.as_slice(), row_to_note)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// M2 — pending hand-offs awaiting receipt by `to_agent`. Excludes retired and
/// already-acknowledged rows.
pub fn list_pending_handoffs(conn: &Connection, to_agent: &str) -> DomainResult<Vec<AgentNote>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM agent_notes \
             WHERE kind = 'handoff' AND to_agent = ?1 \
               AND receipt_acknowledged_at IS NULL AND retired_at IS NULL \
             ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([to_agent], row_to_note)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn ack_handoff(conn: &Connection, id: &Id) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE agent_notes SET receipt_acknowledged_at = ?1 \
             WHERE id = ?2 AND kind = 'handoff' AND receipt_acknowledged_at IS NULL",
            params![fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

pub fn retire(conn: &Connection, id: &Id, reason: &str) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE agent_notes SET retired_at = ?1, retired_reason = ?2 \
             WHERE id = ?3 AND retired_at IS NULL",
            params![fmt_ts(now()), reason, id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{plan as plan_repo, project as proj_repo};
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::plan::{Plan, PlanStatus};
    use sdi_core::project::Project;

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-anote-{}-{}.db",
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
        let plan = Plan {
            id: Id::new(IdKind::Plan),
            project_id: proj.id.clone(),
            short_code: format!("TST-{}", ulid::Ulid::new()),
            title: "v".into(),
            body: "".into(),
            status: PlanStatus::Draft,
            version: 0,
            approved_at: None,
            completed_at: None,
            created_at: now(),
            updated_at: now(),
        };
        plan_repo::insert(&conn, &plan).unwrap();
        (pool, proj.id, plan.id)
    }

    fn mk_plan_note(
        project_id: Id,
        plan_id: Id,
        kind: AgentNoteKind,
        to: Option<String>,
    ) -> AgentNote {
        AgentNote {
            id: Id::new(IdKind::AgentNote),
            project_id,
            plan_id: Some(plan_id),
            round_id: None,
            scenario_id: None,
            task_id: None,
            scope_kind: AgentNoteScope::Plan,
            kind,
            from_agent: "impl-coder".into(),
            to_agent: to,
            body: "ping".into(),
            payload: serde_json::json!({}),
            receipt_acknowledged_at: None,
            retired_at: None,
            retired_reason: None,
            created_at: now(),
        }
    }

    #[test]
    fn handoff_pending_then_ack_clears_queue() {
        let (pool, project_id, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let note = mk_plan_note(
            project_id,
            plan_id,
            AgentNoteKind::Handoff,
            Some("test-runner".into()),
        );
        insert(&conn, &note).unwrap();
        let pending = list_pending_handoffs(&conn, "test-runner").unwrap();
        assert_eq!(pending.len(), 1);
        ack_handoff(&conn, &note.id).unwrap();
        let pending = list_pending_handoffs(&conn, "test-runner").unwrap();
        assert!(pending.is_empty());
    }

    #[test]
    fn retire_excludes_from_active_list() {
        let (pool, project_id, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let note = mk_plan_note(project_id, plan_id.clone(), AgentNoteKind::Observation, None);
        insert(&conn, &note).unwrap();
        assert_eq!(
            list_active(&conn, AgentNoteScope::Plan, &plan_id, None).unwrap().len(),
            1
        );
        retire(&conn, &note.id, "noise").unwrap();
        assert!(list_active(&conn, AgentNoteScope::Plan, &plan_id, None).unwrap().is_empty());
    }

    #[test]
    fn handoff_without_to_agent_rejected_before_sql() {
        let (pool, project_id, plan_id) = fixture();
        let conn = pool.get().unwrap();
        let bad = mk_plan_note(project_id, plan_id, AgentNoteKind::Handoff, None);
        let err = insert(&conn, &bad).unwrap_err();
        assert!(matches!(err, DomainError::Validation(_)));
    }
}
