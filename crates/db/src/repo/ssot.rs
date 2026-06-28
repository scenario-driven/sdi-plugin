//! SSoT product-definition graph repository (PRD-v2 D32). Nodes + edges with the
//! two completeness queries the deterministic verify (D34) consumes: facet
//! completeness (`count_incomplete_nodes`) and link completeness (`count_dangling_edges`).

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::ssot::{Confidence, SsotEdge, SsotNode};

// ---------------------------------------------------------------- nodes

fn row_to_node(row: &Row<'_>) -> rusqlite::Result<SsotNode> {
    Ok(SsotNode {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        kind: s(row, 3)?,
        title: s(row, 4)?,
        facets_json: s(row, 5)?,
        open_markers_json: s(row, 6)?,
        confidence: parsed::<Confidence>(row, 7)?,
        produced_via_pattern_id: s_opt(row, 8)?,
        created_at: ts(row, 9)?,
        updated_at: ts(row, 10)?,
    })
}

const NODE_COLS: &str = "id, project_id, short_code, kind, title, facets_json, \
                         open_markers_json, confidence, produced_via_pattern_id, \
                         created_at, updated_at";

pub fn insert_node(conn: &Connection, node: &SsotNode) -> DomainResult<()> {
    SsotNode::validate_header(&node.kind, &node.title)?;
    SsotNode::parse_facets(&node.facets_json)?;
    SsotNode::parse_open_markers(&node.open_markers_json)?;
    conn.execute(
        &format!("INSERT INTO ssot_nodes({NODE_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"),
        params![
            node.id.as_str(),
            node.project_id.as_str(),
            node.short_code,
            node.kind,
            node.title,
            node.facets_json,
            node.open_markers_json,
            node.confidence.to_string(),
            node.produced_via_pattern_id,
            fmt_ts(node.created_at),
            fmt_ts(node.updated_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get_node(conn: &Connection, id: &Id) -> DomainResult<SsotNode> {
    conn.query_row(
        &format!("SELECT {NODE_COLS} FROM ssot_nodes WHERE id = ?1"),
        [id.as_str()],
        row_to_node,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_nodes_by_project(conn: &Connection, project_id: &Id) -> DomainResult<Vec<SsotNode>> {
    query_nodes(
        conn,
        &format!("SELECT {NODE_COLS} FROM ssot_nodes WHERE project_id = ?1 ORDER BY created_at"),
        params![project_id.as_str()],
    )
}

pub fn list_nodes_by_kind(
    conn: &Connection,
    project_id: &Id,
    kind: &str,
) -> DomainResult<Vec<SsotNode>> {
    query_nodes(
        conn,
        &format!(
            "SELECT {NODE_COLS} FROM ssot_nodes WHERE project_id = ?1 AND kind = ?2 ORDER BY created_at"
        ),
        params![project_id.as_str(), kind],
    )
}

fn query_nodes(
    conn: &Connection,
    sql: &str,
    p: impl rusqlite::Params,
) -> DomainResult<Vec<SsotNode>> {
    let mut stmt = conn.prepare(sql).map_err(map_sqlite_err)?;
    let rows = stmt.query_map(p, row_to_node).map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// Overwrite a node's facets + open markers (e.g. when an answer fills a blank).
pub fn update_node_facets(
    conn: &Connection,
    id: &Id,
    facets_json: &str,
    open_markers_json: &str,
) -> DomainResult<()> {
    SsotNode::parse_facets(facets_json)?;
    SsotNode::parse_open_markers(open_markers_json)?;
    let n = conn
        .execute(
            "UPDATE ssot_nodes SET facets_json = ?1, open_markers_json = ?2, updated_at = ?3 \
             WHERE id = ?4",
            params![facets_json, open_markers_json, fmt_ts(now()), id.as_str()],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

/// D34 facet completeness — number of nodes carrying an unresolved OPEN marker.
pub fn count_incomplete_nodes(conn: &Connection, project_id: &Id) -> DomainResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM ssot_nodes \
         WHERE project_id = ?1 AND TRIM(open_markers_json) != '[]'",
        [project_id.as_str()],
        |r| r.get(0),
    )
    .map_err(map_sqlite_err)
}

// ---------------------------------------------------------------- edges

fn row_to_edge(row: &Row<'_>) -> rusqlite::Result<SsotEdge> {
    Ok(SsotEdge {
        id: Id::from(s(row, 0)?),
        project_id: Id::from(s(row, 1)?),
        from_node: Id::from(s(row, 2)?),
        to_ref: s(row, 3)?,
        rel: s(row, 4)?,
        created_at: ts(row, 5)?,
    })
}

const EDGE_COLS: &str = "id, project_id, from_node, to_ref, rel, created_at";

pub fn insert_edge(conn: &Connection, edge: &SsotEdge) -> DomainResult<()> {
    if edge.rel.trim().is_empty() || edge.to_ref.trim().is_empty() {
        return Err(DomainError::Validation(
            "ssot edge rel and to_ref must be non-empty".into(),
        ));
    }
    conn.execute(
        &format!("INSERT INTO ssot_edges({EDGE_COLS}) VALUES (?1,?2,?3,?4,?5,?6)"),
        params![
            edge.id.as_str(),
            edge.project_id.as_str(),
            edge.from_node.as_str(),
            edge.to_ref,
            edge.rel,
            fmt_ts(edge.created_at),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn list_edges_by_project(conn: &Connection, project_id: &Id) -> DomainResult<Vec<SsotEdge>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {EDGE_COLS} FROM ssot_edges WHERE project_id = ?1 ORDER BY created_at"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([project_id.as_str()], row_to_edge)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// D34 link completeness — number of edges whose `to_ref` resolves to no node
/// (by id or short_code) in the project. Zero = link-complete.
pub fn count_dangling_edges(conn: &Connection, project_id: &Id) -> DomainResult<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM ssot_edges e \
         WHERE e.project_id = ?1 AND NOT EXISTS ( \
            SELECT 1 FROM ssot_nodes n \
            WHERE n.project_id = ?1 AND (n.id = e.to_ref OR n.short_code = e.to_ref) \
         )",
        [project_id.as_str()],
        |r| r.get(0),
    )
    .map_err(map_sqlite_err)
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
            "sdi-repo-ssot-{}-{}.db",
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

    fn mk_node(project_id: Id, kind: &str, open: &str) -> SsotNode {
        SsotNode {
            id: Id::new(IdKind::SsotNode),
            project_id,
            short_code: format!("SN-{}", ulid::Ulid::new()),
            kind: kind.into(),
            title: "t".into(),
            facets_json: "{}".into(),
            open_markers_json: open.into(),
            confidence: Confidence::Unverified,
            produced_via_pattern_id: None,
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn node_roundtrip_and_incomplete_count() {
        let (pool, pid) = fixture();
        let conn = pool.get().unwrap();
        let complete = mk_node(pid.clone(), "Persona", "[]");
        let incomplete = mk_node(
            pid.clone(),
            "Capability",
            r#"[{"id":"m1","field":"definition","description":"확정 필요"}]"#,
        );
        insert_node(&conn, &complete).unwrap();
        insert_node(&conn, &incomplete).unwrap();
        assert_eq!(get_node(&conn, &complete.id).unwrap().kind, "Persona");
        assert_eq!(list_nodes_by_project(&conn, &pid).unwrap().len(), 2);
        assert_eq!(list_nodes_by_kind(&conn, &pid, "Persona").unwrap().len(), 1);
        assert_eq!(count_incomplete_nodes(&conn, &pid).unwrap(), 1);
    }

    #[test]
    fn dangling_edge_counted_until_target_exists() {
        let (pool, pid) = fixture();
        let conn = pool.get().unwrap();
        let from = mk_node(pid.clone(), "Capability", "[]");
        insert_node(&conn, &from).unwrap();
        let edge = SsotEdge {
            id: Id::new(IdKind::SsotEdge),
            project_id: pid.clone(),
            from_node: from.id.clone(),
            to_ref: "SN-ghost".into(),
            rel: "servesPersona".into(),
            created_at: now(),
        };
        insert_edge(&conn, &edge).unwrap();
        assert_eq!(count_dangling_edges(&conn, &pid).unwrap(), 1);
        // Add the target by short_code → no longer dangling.
        let mut target = mk_node(pid.clone(), "Persona", "[]");
        target.short_code = "SN-ghost".into();
        insert_node(&conn, &target).unwrap();
        assert_eq!(count_dangling_edges(&conn, &pid).unwrap(), 0);
    }
}
