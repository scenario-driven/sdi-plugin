//! Decision-question engine repository (PRD-v2 D35). Questions + options +
//! answers. `count_open_questions` feeds the "no unanswered question" half of
//! the spec-convergence completeness gate (the other half — "no unasked
//! question" — is the engine's loop-until-dry, not a stored count).

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::decision_question::{
    DecisionQuestion, QuestionAnswer, QuestionOption, QuestionStatus, QuestionType,
};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};

// ---------------------------------------------------------------- questions

fn row_to_question(row: &Row<'_>) -> rusqlite::Result<DecisionQuestion> {
    Ok(DecisionQuestion {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        scope_ref: s_opt(row, 3)?,
        qtype: parsed::<QuestionType>(row, 4)?,
        context_md: s(row, 5)?,
        parent_question_id: s_opt(row, 6)?,
        status: parsed::<QuestionStatus>(row, 7)?,
        created_at: ts(row, 8)?,
        updated_at: ts(row, 9)?,
    })
}

const Q_COLS: &str = "id, project_id, short_code, scope_ref, qtype, context_md, \
                      parent_question_id, status, created_at, updated_at";

pub fn insert_question(conn: &Connection, q: &DecisionQuestion) -> DomainResult<()> {
    DecisionQuestion::validate_context(&q.context_md)?;
    conn.execute(
        &format!(
            "INSERT INTO decision_questions({Q_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
        ),
        params![
            q.id.as_str(),
            q.project_id.as_str(),
            q.short_code,
            q.scope_ref,
            q.qtype.to_string(),
            q.context_md,
            q.parent_question_id,
            q.status.to_string(),
            fmt_ts(q.created_at),
            fmt_ts(q.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get_question(conn: &Connection, id: &Id) -> DomainResult<DecisionQuestion> {
    conn.query_row(
        &format!("SELECT {Q_COLS} FROM decision_questions WHERE id = ?1"),
        [id.as_str()],
        row_to_question,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_questions_by_project(
    conn: &Connection,
    project_id: &Id,
) -> DomainResult<Vec<DecisionQuestion>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {Q_COLS} FROM decision_questions WHERE project_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], row_to_question)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn set_question_status(conn: &Connection, id: &Id, status: QuestionStatus) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE decision_questions SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_string(), fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Spec-convergence gate input — questions still `open` (not answered/auto-decided).
pub fn count_open_questions(conn: &Connection, project_id: &Id) -> DomainResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM decision_questions WHERE project_id = ?1 AND status = 'open'",
        [project_id.as_str()],
        |r| r.get(0),
    )
    .map_err(map_sqlite_err)
}

/// D34 — count `open` questions scoped to any of the given refs (a plan id or
/// its targeted flow ids). The approve gate blocks while plan-scoped blanks remain.
pub fn count_open_scoped(conn: &Connection, refs: &[String]) -> DomainResult<i64> {
    if refs.is_empty() {
        return Ok(0);
    }
    let placeholders = (1..=refs.len())
        .map(|i| format!("?{i}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT COUNT(*) FROM decision_questions \
         WHERE status = 'open' AND scope_ref IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
    let n: i64 = stmt
        .query_row(rusqlite::params_from_iter(refs.iter()), |r| r.get(0))
        .map_err(map_sqlite_err)?;
    Ok(n)
}

// ---------------------------------------------------------------- options

fn row_to_option(row: &Row<'_>) -> rusqlite::Result<QuestionOption> {
    Ok(QuestionOption {
        id: Id::from(s(row, 0)?),
        question_id: Id::from(s(row, 1)?),
        label: s(row, 2)?,
        body_md: s(row, 3)?,
        rationale_md: s(row, 4)?,
        is_llm_recommended: row.get::<_, i64>(5)? != 0,
        idx: row.get(6)?,
        created_at: ts(row, 7)?,
    })
}

const O_COLS: &str =
    "id, question_id, label, body_md, rationale_md, is_llm_recommended, idx, created_at";

pub fn insert_option(conn: &Connection, opt: &QuestionOption) -> DomainResult<()> {
    if opt.label.trim().is_empty() {
        return Err(DomainError::Validation(
            "option label must be non-empty".into(),
        ));
    }
    conn.execute(
        &format!("INSERT INTO question_options({O_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)"),
        params![
            opt.id.as_str(),
            opt.question_id.as_str(),
            opt.label,
            opt.body_md,
            opt.rationale_md,
            opt.is_llm_recommended as i64,
            opt.idx,
            fmt_ts(opt.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn list_options(conn: &Connection, question_id: &Id) -> DomainResult<Vec<QuestionOption>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {O_COLS} FROM question_options WHERE question_id = ?1 ORDER BY idx, created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([question_id.as_str()], row_to_option)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

// ---------------------------------------------------------------- answers

fn row_to_answer(row: &Row<'_>) -> rusqlite::Result<QuestionAnswer> {
    Ok(QuestionAnswer {
        id: Id::from(s(row, 0)?),
        question_id: Id::from(s(row, 1)?),
        chosen_option_id: s_opt(row, 2)?,
        free_text: s_opt(row, 3)?,
        answered_by: s(row, 4)?,
        generated_refs_json: s(row, 5)?,
        created_at: ts(row, 6)?,
    })
}

const A_COLS: &str =
    "id, question_id, chosen_option_id, free_text, answered_by, generated_refs_json, created_at";

/// Record an answer and flip the question to its terminal status atomically.
/// `auto` (fact-type, 1 survivor) → `auto_decided`; otherwise → `answered`.
pub fn insert_answer(conn: &Connection, ans: &QuestionAnswer, auto: bool) -> DomainResult<()> {
    QuestionAnswer::validate(&ans.chosen_option_id, &ans.free_text)?;
    conn.execute(
        &format!("INSERT INTO question_answers({A_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7)"),
        params![
            ans.id.as_str(),
            ans.question_id.as_str(),
            ans.chosen_option_id,
            ans.free_text,
            ans.answered_by,
            ans.generated_refs_json,
            fmt_ts(ans.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    let status = if auto {
        QuestionStatus::AutoDecided
    } else {
        QuestionStatus::Answered
    };
    set_question_status(conn, &ans.question_id, status)?;
    Ok(())
}

pub fn get_answer(conn: &Connection, question_id: &Id) -> DomainResult<Option<QuestionAnswer>> {
    match conn.query_row(
        &format!(
            "SELECT {A_COLS} FROM question_answers WHERE question_id = ?1 ORDER BY created_at DESC LIMIT 1"
        ),
        [question_id.as_str()],
        row_to_answer,
    ) {
        Ok(a) => Ok(Some(a)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::project as proj_repo;
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::project::Project;

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-dq-{}-{}.db",
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
            description: None,
            enabled: true,
            wiki_paths: vec!["docs".into()],
            created_at: now(),
            updated_at: now(),
        };
        proj_repo::insert(&conn, &proj).unwrap();
        (pool, proj.id)
    }

    fn mk_q(project_id: Id) -> DecisionQuestion {
        DecisionQuestion {
            id: Id::new(IdKind::DecisionQuestion),
            project_id,
            short_code: format!("DQ-{}", ulid::Ulid::new()),
            scope_ref: None,
            qtype: QuestionType::Preference,
            context_md: "페르소나 X가 결제 실패 시 어떻게 동작해야 하나".into(),
            parent_question_id: None,
            status: QuestionStatus::Open,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn question_option_answer_flow() {
        let (pool, pid) = fixture();
        let conn = pool.get().unwrap();
        let q = mk_q(pid.clone());
        insert_question(&conn, &q).unwrap();
        assert_eq!(count_open_questions(&conn, &pid).unwrap(), 1);

        let opt = QuestionOption {
            id: Id::new(IdKind::QuestionOption),
            question_id: q.id.clone(),
            label: "재시도 유도".into(),
            body_md: "".into(),
            rationale_md: "이탈 최소화".into(),
            is_llm_recommended: true,
            idx: 0,
            created_at: now(),
        };
        insert_option(&conn, &opt).unwrap();
        let opts = list_options(&conn, &q.id).unwrap();
        assert_eq!(opts.len(), 1);
        assert!(opts[0].is_llm_recommended);

        let ans = QuestionAnswer {
            id: Id::new(IdKind::QuestionAnswer),
            question_id: q.id.clone(),
            chosen_option_id: Some(opt.id.as_str().into()),
            free_text: None,
            answered_by: "user".into(),
            generated_refs_json: "[]".into(),
            created_at: now(),
        };
        insert_answer(&conn, &ans, false).unwrap();
        assert_eq!(count_open_questions(&conn, &pid).unwrap(), 0);
        assert_eq!(
            get_question(&conn, &q.id).unwrap().status,
            QuestionStatus::Answered
        );
        assert!(get_answer(&conn, &q.id).unwrap().is_some());
    }

    #[test]
    fn answer_without_choice_or_text_rejected() {
        let (pool, pid) = fixture();
        let conn = pool.get().unwrap();
        let q = mk_q(pid);
        insert_question(&conn, &q).unwrap();
        let ans = QuestionAnswer {
            id: Id::new(IdKind::QuestionAnswer),
            question_id: q.id.clone(),
            chosen_option_id: None,
            free_text: None,
            answered_by: "user".into(),
            generated_refs_json: "[]".into(),
            created_at: now(),
        };
        assert!(matches!(
            insert_answer(&conn, &ans, false),
            Err(DomainError::Validation(_))
        ));
    }
}
