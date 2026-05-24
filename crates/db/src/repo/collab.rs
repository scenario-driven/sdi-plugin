//! Repository for collaboration entities (comments, questions, activity).
//! Three small surfaces colocated to keep churn diff-localized as the schema
//! evolves. Each section is fully independent.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::collab::{Activity, Comment, Question, QuestionStatus};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};

// ---------------------------------------------------------------------------
// comments
// ---------------------------------------------------------------------------

const COMMENT_COLS: &str =
    "id, project_id, plan_id, task_id, scenario_id, round_id, author, body, created_at, updated_at";

fn row_to_comment(row: &Row<'_>) -> rusqlite::Result<Comment> {
    Ok(Comment {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        plan_id: s_opt(row, 2)?.map(Id::from),
        task_id: s_opt(row, 3)?.map(Id::from),
        scenario_id: s_opt(row, 4)?.map(Id::from),
        round_id: s_opt(row, 5)?.map(Id::from),
        author: s(row, 6)?,
        body: s(row, 7)?,
        created_at: ts(row, 8)?,
        updated_at: ts(row, 9)?,
    })
}

pub fn insert_comment(conn: &Connection, c: &Comment) -> DomainResult<()> {
    c.validate_anchor()?;
    conn.execute(
        &format!("INSERT INTO comments({COMMENT_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
        params![
            c.id.as_str(),
            c.project_id.as_str(),
            c.plan_id.as_ref().map(|i| i.as_str()),
            c.task_id.as_ref().map(|i| i.as_str()),
            c.scenario_id.as_ref().map(|i| i.as_str()),
            c.round_id.as_ref().map(|i| i.as_str()),
            c.author,
            c.body,
            fmt_ts(c.created_at),
            fmt_ts(c.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get_comment(conn: &Connection, id: &Id) -> DomainResult<Comment> {
    conn.query_row(
        &format!("SELECT {COMMENT_COLS} FROM comments WHERE id = ?1"),
        [id.as_str()],
        row_to_comment,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

/// List comments anchored to a single entity. Caller picks the column to
/// filter on; only one of `plan_id`/`task_id`/`scenario_id`/`round_id` is
/// non-null on any given row.
pub fn list_comments_by(
    conn: &Connection,
    anchor_col: &str,
    anchor_id: &Id,
) -> DomainResult<Vec<Comment>> {
    if !matches!(
        anchor_col,
        "plan_id" | "task_id" | "scenario_id" | "round_id"
    ) {
        return Err(DomainError::Validation(format!(
            "invalid comment anchor column: {anchor_col}"
        )));
    }
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COMMENT_COLS} FROM comments WHERE {anchor_col} = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([anchor_id.as_str()], row_to_comment)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn list_comments_by_project(conn: &Connection, project_id: &Id) -> DomainResult<Vec<Comment>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COMMENT_COLS} FROM comments WHERE project_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], row_to_comment)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn update_comment_body(conn: &Connection, id: &Id, body: &str) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE comments SET body = ?1, updated_at = ?2 WHERE id = ?3",
            params![body, fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

pub fn delete_comment(conn: &Connection, id: &Id) -> DomainResult<()> {
    let n = conn
        .execute("DELETE FROM comments WHERE id = ?1", [id.as_str()])
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// questions
// ---------------------------------------------------------------------------

const QUESTION_COLS: &str =
    "id, plan_id, asker, body, answer, answered_by, answered_at, status, created_at, updated_at";

fn row_to_question(row: &Row<'_>) -> rusqlite::Result<Question> {
    Ok(Question {
        id: Id::from(s(row, 0)?),
        plan_id: Id::from(s(row, 1)?),
        asker: s(row, 2)?,
        body: s(row, 3)?,
        answer: s_opt(row, 4)?,
        answered_by: s_opt(row, 5)?,
        answered_at: ts_opt(row, 6)?,
        status: parsed::<QuestionStatus>(row, 7)?,
        created_at: ts(row, 8)?,
        updated_at: ts(row, 9)?,
    })
}

pub fn insert_question(conn: &Connection, q: &Question) -> DomainResult<()> {
    conn.execute(
        &format!("INSERT INTO questions({QUESTION_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
        params![
            q.id.as_str(),
            q.plan_id.as_str(),
            q.asker,
            q.body,
            q.answer,
            q.answered_by,
            q.answered_at.map(fmt_ts),
            q.status.to_string(),
            fmt_ts(q.created_at),
            fmt_ts(q.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get_question(conn: &Connection, id: &Id) -> DomainResult<Question> {
    conn.query_row(
        &format!("SELECT {QUESTION_COLS} FROM questions WHERE id = ?1"),
        [id.as_str()],
        row_to_question,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_questions_by_plan(
    conn: &Connection,
    plan_id: &Id,
    status: Option<QuestionStatus>,
) -> DomainResult<Vec<Question>> {
    let (sql, params): (String, Vec<String>) = match status {
        Some(st) => (
            format!(
                "SELECT {QUESTION_COLS} FROM questions WHERE plan_id = ?1 AND status = ?2 \
                 ORDER BY created_at"
            ),
            vec![plan_id.to_string(), st.to_string()],
        ),
        None => (
            format!("SELECT {QUESTION_COLS} FROM questions WHERE plan_id = ?1 ORDER BY created_at"),
            vec![plan_id.to_string()],
        ),
    };
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), row_to_question)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn answer_question(
    conn: &Connection,
    id: &Id,
    answer: &str,
    answered_by: &str,
) -> DomainResult<()> {
    let now_ts = fmt_ts(now());
    let n = conn
        .execute(
            "UPDATE questions SET answer = ?1, answered_by = ?2, answered_at = ?3, \
             status = 'answered', updated_at = ?3 WHERE id = ?4 AND status = 'open'",
            params![answer, answered_by, now_ts, id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::Validation(format!(
            "question {id} not found or already answered"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// activity
// ---------------------------------------------------------------------------

const ACTIVITY_COLS: &str = "id, project_id, actor, kind, entity_id, summary, payload, created_at";

fn row_to_activity(row: &Row<'_>) -> rusqlite::Result<Activity> {
    let payload_raw: String = row.get(6)?;
    let payload = serde_json::from_str(&payload_raw).unwrap_or(serde_json::Value::Null);
    Ok(Activity {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        actor: s(row, 2)?,
        kind: s(row, 3)?,
        entity_id: s_opt(row, 4)?.map(Id::from),
        summary: s(row, 5)?,
        payload,
        created_at: ts(row, 7)?,
    })
}

pub fn insert_activity(conn: &Connection, a: &Activity) -> DomainResult<()> {
    conn.execute(
        &format!("INSERT INTO activity({ACTIVITY_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"),
        params![
            a.id.as_str(),
            a.project_id.as_str(),
            a.actor,
            a.kind,
            a.entity_id.as_ref().map(|i| i.as_str()),
            a.summary,
            serde_json::to_string(&a.payload).unwrap_or_else(|_| "{}".into()),
            fmt_ts(a.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn list_activity_by_project(
    conn: &Connection,
    project_id: &Id,
    limit: usize,
) -> DomainResult<Vec<Activity>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {ACTIVITY_COLS} FROM activity WHERE project_id = ?1 \
             ORDER BY created_at DESC LIMIT ?2"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(params![project_id.as_str(), limit as i64], row_to_activity)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn list_activity_stats(conn: &Connection, project_id: &Id) -> DomainResult<Vec<(String, i64)>> {
    let mut stmt = conn
        .prepare(
            "SELECT kind, COUNT(*) FROM activity WHERE project_id = ?1 \
             GROUP BY kind ORDER BY 2 DESC",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}
