//! Knowledge repository — RAG-scoped local memory. Only `scope='rag'` is
//! exposed via the MCP server per PRD §5.4; `reference` and `archive` stay
//! visible to CLI/web but invisible to LLM tools.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, json, parsed, s, s_opt, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::knowledge::{Knowledge, KnowledgeScope};

fn row_to_knowledge(row: &Row<'_>) -> rusqlite::Result<Knowledge> {
    Ok(Knowledge {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        scope: parsed::<KnowledgeScope>(row, 2)?,
        kind: s(row, 3)?,
        title: s(row, 4)?,
        body: s(row, 5)?,
        tags: json::<Vec<String>>(row, 6)?,
        source_ref: s_opt(row, 7)?,
        created_at: ts(row, 8)?,
        updated_at: ts(row, 9)?,
    })
}

const COLS: &str =
    "id, project_id, scope, kind, title, body, tags, source_ref, created_at, updated_at";

pub fn insert(conn: &Connection, k: &Knowledge) -> DomainResult<()> {
    let tags = serde_json::to_string(&k.tags)
        .map_err(|e| DomainError::Validation(format!("encode tags: {e}")))?;
    conn.execute(
        &format!("INSERT INTO knowledge({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"),
        params![
            k.id.as_str(),
            k.project_id.as_str(),
            k.scope.to_string(),
            k.kind,
            k.title,
            k.body,
            tags,
            k.source_ref,
            fmt_ts(k.created_at),
            fmt_ts(k.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Knowledge> {
    conn.query_row(
        &format!("SELECT {COLS} FROM knowledge WHERE id = ?1"),
        [id.as_str()],
        row_to_knowledge,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_project(
    conn: &Connection,
    project_id: &Id,
    scope: Option<KnowledgeScope>,
) -> DomainResult<Vec<Knowledge>> {
    let (sql, args): (String, Vec<String>) = match scope {
        Some(s) => (
            format!(
                "SELECT {COLS} FROM knowledge WHERE project_id = ?1 AND scope = ?2 \
                 ORDER BY updated_at DESC"
            ),
            vec![project_id.as_str().to_string(), s.to_string()],
        ),
        None => (
            format!("SELECT {COLS} FROM knowledge WHERE project_id = ?1 ORDER BY updated_at DESC"),
            vec![project_id.as_str().to_string()],
        ),
    };
    let mut stmt = conn.prepare(&sql).map_err(map_sqlite_err)?;
    let arg_refs: Vec<&dyn rusqlite::ToSql> =
        args.iter().map(|a| a as &dyn rusqlite::ToSql).collect();
    let rows = stmt
        .query_map(arg_refs.as_slice(), row_to_knowledge)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

pub fn update_body(
    conn: &Connection,
    id: &Id,
    title: &str,
    body: &str,
    tags: &[String],
) -> DomainResult<()> {
    let tags_s = serde_json::to_string(tags)
        .map_err(|e| DomainError::Validation(format!("encode tags: {e}")))?;
    let n = conn
        .execute(
            "UPDATE knowledge SET title = ?1, body = ?2, tags = ?3, updated_at = ?4 \
             WHERE id = ?5",
            params![title, body, tags_s, fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

pub fn delete(conn: &Connection, id: &Id) -> DomainResult<()> {
    let n = conn
        .execute("DELETE FROM knowledge WHERE id = ?1", [id.as_str()])
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

/// FTS5-backed search restricted to a single scope (rag for MCP).
pub fn search(
    conn: &Connection,
    project_id: &Id,
    scope: KnowledgeScope,
    query: &str,
    limit: usize,
) -> DomainResult<Vec<Knowledge>> {
    let mut stmt = conn
        .prepare(
            "SELECT k.id, k.project_id, k.scope, k.kind, k.title, k.body, k.tags, \
                    k.source_ref, k.created_at, k.updated_at \
             FROM knowledge k JOIN knowledge_fts f ON f.rowid = k.rowid \
             WHERE k.project_id = ?1 AND k.scope = ?2 AND knowledge_fts MATCH ?3 \
             ORDER BY rank LIMIT ?4",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(
            params![project_id.as_str(), scope.to_string(), query, limit as i64],
            row_to_knowledge,
        )
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

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-kn-{}-{}.db",
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
        (pool, proj.id)
    }

    fn mk(project_id: Id, scope: KnowledgeScope, body: &str) -> Knowledge {
        Knowledge {
            id: Id::new(IdKind::Knowledge),
            project_id,
            scope,
            kind: "decision".into(),
            title: "Adopt FTS5".into(),
            body: body.into(),
            tags: vec!["search".into(), "sqlite".into()],
            source_ref: Some("PRD §5.2".into()),
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn scope_filter_isolates_rag_from_reference() {
        let (pool, project_id) = fixture();
        let conn = pool.get().unwrap();
        insert(
            &conn,
            &mk(project_id.clone(), KnowledgeScope::Rag, "rag note"),
        )
        .unwrap();
        insert(
            &conn,
            &mk(project_id.clone(), KnowledgeScope::Reference, "ref note"),
        )
        .unwrap();
        let rag = list_by_project(&conn, &project_id, Some(KnowledgeScope::Rag)).unwrap();
        let r#ref = list_by_project(&conn, &project_id, Some(KnowledgeScope::Reference)).unwrap();
        let all = list_by_project(&conn, &project_id, None).unwrap();
        assert_eq!(rag.len(), 1);
        assert_eq!(r#ref.len(), 1);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn search_finds_within_scope() {
        let (pool, project_id) = fixture();
        let conn = pool.get().unwrap();
        insert(
            &conn,
            &mk(
                project_id.clone(),
                KnowledgeScope::Rag,
                "fts5 keyword search",
            ),
        )
        .unwrap();
        insert(
            &conn,
            &mk(
                project_id.clone(),
                KnowledgeScope::Reference,
                "fts5 reference doc",
            ),
        )
        .unwrap();
        let hits = search(&conn, &project_id, KnowledgeScope::Rag, "fts5", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].scope, KnowledgeScope::Rag);
    }
}
