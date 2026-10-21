//! Round repository — round lifecycle + per-round scenario verdicts.
//!
//! `start_round_with_carryover` implements PRD §6.3 auto-regression: when a
//! new round is activated under `strict-regression`, every confirmed scenario
//! from the prior round is carried into `scenario_results` with the prior
//! verdict (or 'passing' on first inheritance). Disrupted scenarios surface
//! as 'failing' so the user must re-evaluate.

use crate::map_sqlite_err;
use crate::repo::{fmt_ts, parsed, s, s_opt, ts, ts_opt};
use rusqlite::{params, Connection, Row};
use sdi_core::error::{DomainError, DomainResult};
use sdi_core::ids::{now, Id};
use sdi_core::round::{DisruptionPolicy, InFlightPolicy, Round, RoundMode, RoundStatus};
use sdi_core::scenario::ScenarioResult;
use sdi_core::verification::EvidenceRef;
use std::str::FromStr;

fn row_to_round(row: &Row<'_>) -> rusqlite::Result<Round> {
    Ok(Round {
        id: Id::from(s(row, 0)?),
        plan_id: Id::from(s(row, 1)?),
        short_code: s(row, 2)?,
        round_number: row.get(3)?,
        mode: parsed::<RoundMode>(row, 4)?,
        in_flight_policy: parsed::<InFlightPolicy>(row, 5)?,
        disruption_policy: parsed::<DisruptionPolicy>(row, 6)?,
        status: parsed::<RoundStatus>(row, 7)?,
        activated_at: ts_opt(row, 8)?,
        completed_at: ts_opt(row, 9)?,
        created_at: ts(row, 10)?,
        updated_at: ts(row, 11)?,
        produced_via_pattern_id: s_opt(row, 12)?,
    })
}

const COLS: &str = "id, plan_id, short_code, round_number, mode, in_flight_policy, disruption_policy, status, activated_at, completed_at, created_at, updated_at, produced_via_pattern_id";

pub fn insert(conn: &Connection, round: &Round) -> DomainResult<()> {
    conn.execute(
        &format!("INSERT INTO rounds({COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)"),
        params![
            round.id.as_str(),
            round.plan_id.as_str(),
            round.short_code,
            round.round_number,
            round.mode.to_string(),
            round.in_flight_policy.to_string(),
            round.disruption_policy.to_string(),
            round.status.to_string(),
            round.activated_at.map(fmt_ts),
            round.completed_at.map(fmt_ts),
            fmt_ts(round.created_at),
            fmt_ts(round.updated_at),
            round.produced_via_pattern_id,
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn get(conn: &Connection, id: &Id) -> DomainResult<Round> {
    conn.query_row(
        &format!("SELECT {COLS} FROM rounds WHERE id = ?1"),
        [id.as_str()],
        row_to_round,
    )
    .map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => DomainError::NotFound(id.to_string()),
        other => map_sqlite_err(other),
    })
}

pub fn list_by_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Vec<Round>> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {COLS} FROM rounds WHERE plan_id = ?1 ORDER BY round_number"
        ))
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([plan_id.as_str()], row_to_round)
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// Look up a round by its `short_code` within a plan. Used to resolve the
/// `CHORE-R` round under the per-project maintenance container idempotently (#18).
pub fn find_by_plan_short_code(
    conn: &Connection,
    plan_id: &Id,
    short_code: &str,
) -> DomainResult<Option<Round>> {
    let r = conn.query_row(
        &format!("SELECT {COLS} FROM rounds WHERE plan_id = ?1 AND short_code = ?2"),
        params![plan_id.as_str(), short_code],
        row_to_round,
    );
    match r {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

pub fn find_active_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Option<Round>> {
    let r = conn.query_row(
        &format!("SELECT {COLS} FROM rounds WHERE plan_id = ?1 AND status = 'active'"),
        [plan_id.as_str()],
        row_to_round,
    );
    match r {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

pub fn latest_completed_for_plan(conn: &Connection, plan_id: &Id) -> DomainResult<Option<Round>> {
    let r = conn.query_row(
        &format!(
            "SELECT {COLS} FROM rounds WHERE plan_id = ?1 AND status = 'completed' \
             ORDER BY round_number DESC LIMIT 1"
        ),
        [plan_id.as_str()],
        row_to_round,
    );
    match r {
        Ok(p) => Ok(Some(p)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(map_sqlite_err(e)),
    }
}

pub fn next_round_number(conn: &Connection, plan_id: &Id) -> DomainResult<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(round_number), 0) + 1 FROM rounds WHERE plan_id = ?1",
        [plan_id.as_str()],
        |r| r.get(0),
    )
    .map_err(map_sqlite_err)
}

pub fn set_status(
    conn: &Connection,
    id: &Id,
    status: RoundStatus,
    activated_at: Option<chrono::DateTime<chrono::Utc>>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
) -> DomainResult<()> {
    let n = conn
        .execute(
            "UPDATE rounds SET status = ?1, activated_at = COALESCE(?2, activated_at), \
             completed_at = COALESCE(?3, completed_at), updated_at = ?4 WHERE id = ?5",
            params![
                status.to_string(),
                activated_at.map(fmt_ts),
                completed_at.map(fmt_ts),
                fmt_ts(now()),
                id.as_str(),
            ],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(id.to_string()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// scenario_results — per-round verdicts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ScenarioResultRow {
    pub round_id: Id,
    pub scenario_id: Id,
    pub result: ScenarioResult,
    pub note: Option<String>,
    pub evidence_ref: Option<String>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

pub fn upsert_result(
    conn: &Connection,
    round_id: &Id,
    scenario_id: &Id,
    result: ScenarioResult,
    note: Option<&str>,
    evidence_ref: Option<&str>,
) -> DomainResult<()> {
    conn.execute(
        "INSERT INTO scenario_results(round_id, scenario_id, result, note, evidence_ref, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(round_id, scenario_id) DO UPDATE SET \
            result = excluded.result, note = excluded.note, \
            evidence_ref = excluded.evidence_ref, updated_at = excluded.updated_at",
        params![
            round_id.as_str(),
            scenario_id.as_str(),
            result.to_string(),
            note,
            evidence_ref,
            fmt_ts(now()),
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(())
}

pub fn list_results(conn: &Connection, round_id: &Id) -> DomainResult<Vec<ScenarioResultRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT round_id, scenario_id, result, note, evidence_ref, updated_at \
             FROM scenario_results WHERE round_id = ?1 ORDER BY scenario_id",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map([round_id.as_str()], |r| {
            Ok(ScenarioResultRow {
                round_id: Id::from(s(r, 0)?),
                scenario_id: Id::from(s(r, 1)?),
                result: parsed::<ScenarioResult>(r, 2)?,
                note: s_opt(r, 3)?,
                evidence_ref: s_opt(r, 4)?,
                updated_at: ts(r, 5)?,
            })
        })
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// The round's verification baseline.
///
/// Two halves with different owners:
/// - **activation record** (`commit` + `snapshot`) — written by the daemon
///   when the round activates: the HEAD commit and every confirmed scenario's
///   verdict at that instant. It fixes the denominator of baseline progress.
/// - **caller blob** (`blob`) — the test-runner's own metadata (e.g.
///   `{"green_tests": 412}`), surfaced in `sdi task brief`.
///
/// Once a round has left `planning`, neither half may be overwritten without
/// a superseding decision of the same plan (`decision_id` records the last
/// one). The first write of an empty blob is not an overwrite.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct BaselineRecord {
    pub blob: Option<String>,
    pub commit: Option<String>,
    pub snapshot: Option<serde_json::Value>,
    pub recorded_at: Option<chrono::DateTime<chrono::Utc>>,
    pub decision_id: Option<String>,
}

pub fn get_baseline_record(conn: &Connection, round_id: &Id) -> DomainResult<BaselineRecord> {
    match conn.query_row(
        "SELECT baseline_json, baseline_commit, baseline_snapshot_json, baseline_recorded_at, \
                baseline_decision_id FROM rounds WHERE id = ?1",
        [round_id.as_str()],
        |r| {
            Ok(BaselineRecord {
                blob: s_opt(r, 0)?,
                commit: s_opt(r, 1)?,
                snapshot: s_opt(r, 2)?.and_then(|raw| serde_json::from_str(&raw).ok()),
                recorded_at: ts_opt(r, 3)?,
                decision_id: s_opt(r, 4)?,
            })
        },
    ) {
        Ok(v) => Ok(v),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Err(DomainError::NotFound(round_id.to_string()))
        }
        Err(e) => Err(map_sqlite_err(e)),
    }
}

/// #15 — read the round's caller baseline blob, if any.
pub fn get_baseline(conn: &Connection, round_id: &Id) -> DomainResult<Option<String>> {
    Ok(get_baseline_record(conn, round_id)?.blob)
}

/// Validate that `decision_id` may supersede the baseline of `round`.
fn check_superseding_decision(
    conn: &Connection,
    round: &Round,
    decision_id: &Id,
) -> DomainResult<()> {
    let d = crate::repo::decision::get(conn, decision_id)
        .map_err(|_| DomainError::Validation(format!("decision {decision_id} not found")))?;
    if d.plan_id != round.plan_id {
        return Err(DomainError::Validation(format!(
            "decision {decision_id} belongs to plan {}, not the round's plan {}",
            d.plan_id, round.plan_id
        )));
    }
    if d.status == sdi_core::decision::DecisionStatus::Superseded {
        return Err(DomainError::Validation(format!(
            "decision {decision_id} is itself superseded and cannot supersede a baseline"
        )));
    }
    Ok(())
}

/// #15 — set the round's caller baseline blob. A planning round's blob is
/// freely editable; once the round is active or completed an existing blob
/// is locked and only a superseding decision of the same plan may replace it.
pub fn set_baseline(
    conn: &Connection,
    round_id: &Id,
    baseline_json: &str,
    superseding_decision: Option<&Id>,
) -> DomainResult<()> {
    let round = get(conn, round_id)?;
    let existing = get_baseline_record(conn, round_id)?;
    let locked = round.status != RoundStatus::Planning && existing.blob.is_some();
    match (locked, superseding_decision) {
        (true, None) => {
            return Err(DomainError::BaselineLocked(format!(
                "round {} is {} and already has a baseline; pass a superseding decision id \
                 (`sdi round baseline {} --set … --decision <DEC-ID>`) to replace it",
                round.short_code, round.status, round.id
            )))
        }
        (_, Some(d)) => check_superseding_decision(conn, &round, d)?,
        (false, None) => {}
    }
    let n = conn
        .execute(
            "UPDATE rounds SET baseline_json = ?1, \
                 baseline_decision_id = COALESCE(?2, baseline_decision_id), updated_at = ?3 \
             WHERE id = ?4",
            params![
                baseline_json,
                superseding_decision.map(|d| d.as_str().to_string()),
                fmt_ts(now()),
                round_id.as_str()
            ],
        )
        .map_err(map_sqlite_err)?;
    if n == 0 {
        return Err(DomainError::NotFound(round_id.to_string()));
    }
    Ok(())
}

/// Every confirmed, non-retired scenario of the plan with its verdict in
/// `round_id` (`unverified` when none) — the activation verdict snapshot.
pub fn capture_snapshot(
    conn: &Connection,
    round: &Round,
    commit: Option<&str>,
) -> DomainResult<serde_json::Value> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.short_code, sr.result FROM scenarios s \
             LEFT JOIN scenario_results sr ON sr.scenario_id = s.id AND sr.round_id = ?2 \
             WHERE s.plan_id = ?1 AND s.status = 'confirmed' AND s.retired_at IS NULL \
             ORDER BY s.created_at, s.id",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(params![round.plan_id.as_str(), round.id.as_str()], |r| {
            Ok(serde_json::json!({
                "scenario_id": s(r, 0)?,
                "short_code": s(r, 1)?,
                "verdict": s_opt(r, 2)?.unwrap_or_else(|| "unverified".into()),
            }))
        })
        .map_err(map_sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite_err)?;
    Ok(serde_json::json!({
        "commit": commit,
        "captured_at": fmt_ts(now()),
        "scenarios": rows,
    }))
}

/// Record the activation baseline. Called once, by round activation; a
/// round that already holds an activation record is refused (use
/// [`rebase_baseline`] with a decision instead).
pub fn record_activation_baseline(
    conn: &Connection,
    round: &Round,
    commit: Option<&str>,
) -> DomainResult<serde_json::Value> {
    if get_baseline_record(conn, &round.id)?.snapshot.is_some() {
        return Err(DomainError::BaselineLocked(format!(
            "round {} already recorded its activation baseline",
            round.short_code
        )));
    }
    let snapshot = capture_snapshot(conn, round, commit)?;
    conn.execute(
        "UPDATE rounds SET baseline_commit = ?1, baseline_snapshot_json = ?2, \
             baseline_recorded_at = ?3, updated_at = ?3 WHERE id = ?4",
        params![
            commit,
            serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".into()),
            fmt_ts(now()),
            round.id.as_str()
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(snapshot)
}

/// Re-capture the activation baseline (commit + verdict snapshot) of an
/// active/completed round. Always requires a superseding decision: rebasing
/// changes which scenarios count as "baseline" for progress.
pub fn rebase_baseline(
    conn: &Connection,
    round_id: &Id,
    commit: Option<&str>,
    decision_id: &Id,
) -> DomainResult<serde_json::Value> {
    let round = get(conn, round_id)?;
    check_superseding_decision(conn, &round, decision_id)?;
    let snapshot = capture_snapshot(conn, &round, commit)?;
    conn.execute(
        "UPDATE rounds SET baseline_commit = ?1, baseline_snapshot_json = ?2, \
             baseline_recorded_at = ?3, baseline_decision_id = ?4, updated_at = ?3 WHERE id = ?5",
        params![
            commit,
            serde_json::to_string(&snapshot).unwrap_or_else(|_| "{}".into()),
            fmt_ts(now()),
            decision_id.as_str(),
            round_id.as_str()
        ],
    )
    .map_err(map_sqlite_err)?;
    Ok(snapshot)
}

/// Verdict histogram of one scenario group.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ProgressGroup {
    pub total: usize,
    pub passing: usize,
    pub failing: usize,
    pub impacted: usize,
    pub needs_verification: usize,
    pub unverified: usize,
    /// Baseline scenarios retired since activation — excluded from `total`.
    pub retired: usize,
    /// `passing / total × 100`, one decimal; `null` when the group is empty.
    pub percent: Option<f64>,
    pub scenario_ids: Vec<String>,
}

impl ProgressGroup {
    fn add(&mut self, scenario_id: &str, verdict: Option<ScenarioResult>) {
        if verdict == Some(ScenarioResult::Retired) {
            self.retired += 1;
            return;
        }
        self.total += 1;
        self.scenario_ids.push(scenario_id.to_string());
        match verdict {
            Some(ScenarioResult::Passing) => self.passing += 1,
            Some(ScenarioResult::Failing) => self.failing += 1,
            Some(ScenarioResult::Impacted) => self.impacted += 1,
            Some(ScenarioResult::NeedsVerification) => self.needs_verification += 1,
            Some(ScenarioResult::Retired) => unreachable!("handled above"),
            None => self.unverified += 1,
        }
    }

    fn finish(mut self) -> Self {
        self.percent = (self.total > 0)
            .then(|| ((self.passing as f64 / self.total as f64) * 1000.0).round() / 10.0);
        self
    }
}

/// Fixed-baseline progress: the scenarios in the activation snapshot are
/// measured separately from scenarios that became confirmed afterwards, so
/// adding scenarios mid-round never inflates (or deflates) baseline
/// completion. `baseline` is `None` for rounds activated before the snapshot
/// existed; every scenario then reports under `added_after_baseline`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BaselineProgress {
    pub baseline_commit: Option<String>,
    pub baseline_recorded: bool,
    pub baseline: Option<ProgressGroup>,
    pub added_after_baseline: ProgressGroup,
}

pub fn baseline_progress(conn: &Connection, round: &Round) -> DomainResult<BaselineProgress> {
    let record = get_baseline_record(conn, &round.id)?;
    let verdicts: std::collections::HashMap<String, ScenarioResult> =
        list_results(conn, &round.id)?
            .into_iter()
            .map(|r| (r.scenario_id.to_string(), r.result))
            .collect();
    let baseline_ids: Option<Vec<String>> = record.snapshot.as_ref().map(|snap| {
        snap["scenarios"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|e| e["scenario_id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    });
    let baseline_set: std::collections::HashSet<&str> =
        baseline_ids.iter().flatten().map(String::as_str).collect();

    let mut baseline = ProgressGroup::default();
    if let Some(ids) = baseline_ids.as_ref() {
        for sid in ids {
            // A baseline scenario retired after activation leaves the
            // denominator (it is no longer a requirement), counted as retired.
            let retired_now = crate::repo::scenario::get(conn, &Id::from(sid.as_str()))
                .map(|scn| scn.retired_at.is_some())
                .unwrap_or(true);
            let v = if retired_now {
                Some(ScenarioResult::Retired)
            } else {
                verdicts.get(sid).copied()
            };
            baseline.add(sid, v);
        }
    }

    let mut added = ProgressGroup::default();
    let mut stmt = conn
        .prepare(
            "SELECT id FROM scenarios WHERE plan_id = ?1 AND status = 'confirmed' \
             AND retired_at IS NULL ORDER BY created_at, id",
        )
        .map_err(map_sqlite_err)?;
    let current: Vec<String> = stmt
        .query_map([round.plan_id.as_str()], |r| r.get::<_, String>(0))
        .map_err(map_sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite_err)?;
    for sid in current {
        if !baseline_set.contains(sid.as_str()) {
            let v = verdicts.get(&sid).copied();
            added.add(&sid, v);
        }
    }

    Ok(BaselineProgress {
        baseline_commit: record.commit,
        baseline_recorded: record.snapshot.is_some(),
        baseline: baseline_ids.map(|_| baseline.finish()),
        added_after_baseline: added.finish(),
    })
}

/// One scenario in the round's "needs verification" set (#7).
#[derive(Debug, serde::Serialize)]
pub struct NeedsVerificationRow {
    pub scenario_id: String,
    pub short_code: String,
    pub given: String,
    pub when_clause: String,
    pub then_clause: String,
}

/// #7 — scenarios that still need a fresh verdict in `round_id`: confirmed
/// scenarios in the plan with NO `passing`/`retired` result recorded for this
/// round. That set is exactly { newly-added scenarios (no result row) } ∪
/// { carried-failing / carried-blocked }, the input the LLM decomposes into
/// tasks at activation (SKILL: sdi-round "Task auto-decomposition"). Returned
/// in the round-activate response so the caller never has to recompute it from
/// a full scenario list.
pub fn scenarios_needing_verification(
    conn: &Connection,
    round_id: &Id,
    plan_id: &Id,
) -> DomainResult<Vec<NeedsVerificationRow>> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.short_code, s.given, s.when_clause, s.then_clause \
             FROM scenarios s \
             WHERE s.plan_id = ?1 AND s.status = 'confirmed' AND s.retired_at IS NULL \
               AND NOT EXISTS ( \
                 SELECT 1 FROM scenario_results sr \
                 WHERE sr.round_id = ?2 AND sr.scenario_id = s.id \
                   AND sr.result IN ('passing', 'retired') \
               ) \
             ORDER BY s.created_at",
        )
        .map_err(map_sqlite_err)?;
    let rows = stmt
        .query_map(params![plan_id.as_str(), round_id.as_str()], |r| {
            Ok(NeedsVerificationRow {
                scenario_id: s(r, 0)?,
                short_code: s(r, 1)?,
                given: s(r, 2)?,
                when_clause: s(r, 3)?,
                then_clause: s(r, 4)?,
            })
        })
        .map_err(map_sqlite_err)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(map_sqlite_err)?);
    }
    Ok(out)
}

/// A carried pass demoted to `needs_verification` because its run-ref
/// fingerprint no longer matches the workspace.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DemotedPass {
    pub scenario_id: String,
    pub run_id: String,
    pub recorded_fingerprint: Option<String>,
    pub current_fingerprint: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct CarryOver {
    pub carried: usize,
    pub demoted: Vec<DemotedPass>,
}

/// PRD §6.3 R2+ auto-regression population.
///
/// Carries forward only scenarios that actually received a verdict in the
/// previous round. Scenarios `confirmed` on the plan but with NO prior verdict
/// (i.e. added between rounds, or otherwise unevaluated) are intentionally
/// left absent from the new round's queue: they have never been tested, so
/// silently auto-marking them `passing` would be a false-positive regression.
///
/// Every carried row keeps its evidence reference and note. A `passing`
/// verdict evidenced by a run reference (`run:<RUN-ID>`) is re-checked: the
/// run's input fingerprint is recomputed against the workspace as it is now,
/// and a mismatch (or a fingerprint that can no longer be recomputed) demotes
/// the carried pass to `needs_verification` — a pass proven against other
/// inputs is not evidence for these. Free-text passes carry verbatim (they
/// have no fingerprint to check).
///
/// Disrupted scenarios pre-marked `impacted` by the caller before carry are
/// preserved (the existing-row check below skips overwriting them).
pub fn carry_over_results(
    conn: &Connection,
    new_round: &Round,
    prev_round_id: &Id,
) -> DomainResult<CarryOver> {
    let mut stmt = conn
        .prepare(
            "SELECT sr.scenario_id, sr.result, sr.note, sr.evidence_ref \
             FROM scenario_results sr \
             JOIN scenarios s ON s.id = sr.scenario_id \
             WHERE sr.round_id = ?1 AND s.plan_id = ?2 \
               AND s.status = 'confirmed' AND s.retired_at IS NULL",
        )
        .map_err(map_sqlite_err)?;
    type Prev = (String, String, Option<String>, Option<String>);
    let prev_rows: Vec<Prev> = stmt
        .query_map(
            params![prev_round_id.as_str(), new_round.plan_id.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .map_err(map_sqlite_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite_err)?;
    let mut report = CarryOver::default();
    for (scenario_id, prev_result, prev_note, evidence_ref) in prev_rows {
        let existing: Option<String> = conn
            .query_row(
                "SELECT result FROM scenario_results WHERE round_id = ?1 AND scenario_id = ?2",
                params![new_round.id.as_str(), &scenario_id],
                |r| r.get(0),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(map_sqlite_err(other)),
            })?;
        if existing.is_some() {
            continue;
        }
        let mut carry = ScenarioResult::from_str(&prev_result).map_err(|_| {
            DomainError::Validation(format!(
                "unknown carry-over verdict `{prev_result}` for scenario {scenario_id}",
            ))
        })?;
        let mut note = prev_note;
        if carry == ScenarioResult::Passing {
            let run_ref = evidence_ref
                .as_deref()
                .and_then(|r| EvidenceRef::parse(r).ok())
                .and_then(|r| r.run_id().cloned());
            if let Some(run_id) = run_ref {
                let (fresh, recorded, current, reason) =
                    match crate::repo::run::get_run(conn, &run_id) {
                        Ok(run) => {
                            let f = crate::repo::run::freshness(conn, &run);
                            (f.fresh, f.recorded, f.current, f.reason)
                        }
                        Err(_) => (
                            false,
                            None,
                            None,
                            Some(format!("evidence run {run_id} no longer exists")),
                        ),
                    };
                if !fresh {
                    let reason =
                        reason.unwrap_or_else(|| "fingerprint could not be confirmed".into());
                    carry = ScenarioResult::NeedsVerification;
                    note = Some(match note {
                        Some(n) => format!("carried pass is stale: {reason}; prior note: {n}"),
                        None => format!("carried pass is stale: {reason}"),
                    });
                    report.demoted.push(DemotedPass {
                        scenario_id: scenario_id.clone(),
                        run_id: run_id.to_string(),
                        recorded_fingerprint: recorded,
                        current_fingerprint: current,
                        reason,
                    });
                }
            }
        }
        upsert_result(
            conn,
            &new_round.id,
            &Id::from(scenario_id),
            carry,
            note.as_deref(),
            evidence_ref.as_deref(),
        )?;
        report.carried += 1;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::{plan as plan_repo, project as proj_repo, scenario as scn_repo};
    use crate::{ensure_schema, open_pool};
    use sdi_core::ids::IdKind;
    use sdi_core::plan::{Plan, PlanStatus};
    use sdi_core::project::Project;
    use sdi_core::scenario::{Scenario, ScenarioStatus};

    fn fixture() -> (r2d2::Pool<r2d2_sqlite::SqliteConnectionManager>, Id, Id) {
        let tmp = std::env::temp_dir().join(format!(
            "sdi-repo-round-{}-{}.db",
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
        let plan = Plan {
            id: Id::new(IdKind::Plan),
            project_id: proj.id,
            short_code: format!("TST-{}", ulid::Ulid::new()),
            title: "v".into(),
            body: "".into(),
            status: PlanStatus::Active,
            version: 0,
            produced_via_pattern_id: None,
            approved_at: Some(now()),
            completed_at: None,
            created_at: now(),
            updated_at: now(),
        };
        plan_repo::insert(&conn, &plan).unwrap();
        let scn = Scenario {
            id: Id::new(IdKind::Scenario),
            plan_id: plan.id.clone(),
            short_code: format!("SCN-{}", ulid::Ulid::new()),
            given: "g".into(),
            when_clause: "w".into(),
            then_clause: "t".into(),
            tags: vec![],
            origin_round_id: None,
            status: ScenarioStatus::Confirmed,
            depends_on: vec![],
            produced_by: None,
            verified_by: None,
            claimed_resources_json: "[]".into(),
            claim_status: sdi_core::pattern::ClaimStatus::None,
            produced_via_pattern_id: None,
            retired_at: None,
            belongs_to_flow_id: None,
            covers_flow_step: None,
            created_at: now(),
            updated_at: now(),
        };
        scn_repo::insert(&conn, &scn).unwrap();
        (pool, plan.id, scn.id)
    }

    fn mk_round(plan_id: Id, num: i64, status: RoundStatus) -> Round {
        Round {
            id: Id::new(IdKind::Round),
            plan_id,
            short_code: format!("R-{}", ulid::Ulid::new()),
            round_number: num,
            mode: RoundMode::StrictRegression,
            in_flight_policy: InFlightPolicy::Pause,
            disruption_policy: DisruptionPolicy::NeedsReview,
            status,
            produced_via_pattern_id: None,
            activated_at: if status == RoundStatus::Planning {
                None
            } else {
                Some(now())
            },
            completed_at: if status == RoundStatus::Completed {
                Some(now())
            } else {
                None
            },
            created_at: now(),
            updated_at: now(),
        }
    }

    #[test]
    fn carry_over_passes_default() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Completed);
        let r2 = mk_round(plan_id.clone(), 2, RoundStatus::Active);
        insert(&conn, &r1).unwrap();
        upsert_result(&conn, &r1.id, &scn_id, ScenarioResult::Passing, None, None).unwrap();
        insert(&conn, &r2).unwrap();
        let n = carry_over_results(&conn, &r2, &r1.id).unwrap();
        assert_eq!(n.carried, 1);
        assert!(n.demoted.is_empty());
        let rows = list_results(&conn, &r2.id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].result, ScenarioResult::Passing);
    }

    #[test]
    fn carry_over_preserves_failing() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Completed);
        let r2 = mk_round(plan_id.clone(), 2, RoundStatus::Active);
        insert(&conn, &r1).unwrap();
        upsert_result(&conn, &r1.id, &scn_id, ScenarioResult::Failing, None, None).unwrap();
        insert(&conn, &r2).unwrap();
        carry_over_results(&conn, &r2, &r1.id).unwrap();
        let rows = list_results(&conn, &r2.id).unwrap();
        assert_eq!(rows[0].result, ScenarioResult::Failing);
    }

    #[test]
    fn second_active_round_per_plan_conflicts() {
        let (pool, plan_id, _scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Active);
        let r2 = mk_round(plan_id.clone(), 2, RoundStatus::Active);
        insert(&conn, &r1).unwrap();
        let err = insert(&conn, &r2).unwrap_err();
        assert!(matches!(err, DomainError::Conflict(_)));
    }
    fn git_repo() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sdi-round-git-{}-{}",
            std::process::id(),
            ulid::Ulid::new()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["add", "."],
            vec![
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "i",
            ],
        ] {
            assert!(std::process::Command::new("git")
                .arg("-C")
                .arg(&dir)
                .args(&args)
                .status()
                .unwrap()
                .success());
        }
        dir
    }

    fn passing_run(conn: &Connection, plan_id: &Id, scn_id: &Id, cwd: &std::path::Path) -> Id {
        use sdi_core::run::{Run, RunResult};
        let mut run = Run {
            id: Id::new(IdKind::Run),
            task_id: None,
            plan_id: Some(plan_id.clone()),
            round_id: None,
            scenario_id: Some(scn_id.clone()),
            actor: "agent".into(),
            session_id: None,
            started_at: now(),
            finished_at: Some(now()),
            result: Some(RunResult::Success),
            notes: None,
            payload: serde_json::Value::Null,
            command: Some(vec!["cargo".into(), "test".into()]),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            input_globs: vec!["src/**".into()],
            tool_versions: Default::default(),
            input_fingerprint: None,
            exit_code: Some(0),
            signal: None,
            timed_out: false,
            spawn_error: None,
            report_path: None,
            report_sha256: None,
            failure_class: Some(sdi_core::verification::FailureClass::None),
            failure_signature: None,
            error_class: None,
            failing_test: None,
            duration_ms: Some(1),
        };
        run.input_fingerprint = Some(crate::repo::run::current_fingerprint(conn, &run).unwrap());
        crate::repo::run::insert_run(conn, &run).unwrap();
        run.id
    }

    #[test]
    fn carry_over_rechecks_run_ref_fingerprint_and_keeps_ref() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let repo_dir = git_repo();
        let run_id = passing_run(&conn, &plan_id, &scn_id, &repo_dir);
        let run_ref = format!("run:{run_id}");

        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Completed);
        insert(&conn, &r1).unwrap();
        upsert_result(
            &conn,
            &r1.id,
            &scn_id,
            ScenarioResult::Passing,
            Some("green"),
            Some(&run_ref),
        )
        .unwrap();

        // Unchanged inputs: the pass carries with its ref and note intact.
        let r2 = mk_round(plan_id.clone(), 2, RoundStatus::Completed);
        insert(&conn, &r2).unwrap();
        let rep = carry_over_results(&conn, &r2, &r1.id).unwrap();
        assert!(rep.demoted.is_empty());
        let row = &list_results(&conn, &r2.id).unwrap()[0];
        assert_eq!(row.result, ScenarioResult::Passing);
        assert_eq!(row.evidence_ref.as_deref(), Some(run_ref.as_str()));
        assert_eq!(row.note.as_deref(), Some("green"));

        // An input changes: the carried pass is stale → needs_verification,
        // ref kept, note explains why.
        std::fs::write(repo_dir.join("src/lib.rs"), "pub fn f() { todo!() }\n").unwrap();
        let r3 = mk_round(plan_id.clone(), 3, RoundStatus::Active);
        insert(&conn, &r3).unwrap();
        let rep = carry_over_results(&conn, &r3, &r2.id).unwrap();
        assert_eq!(rep.demoted.len(), 1);
        assert_eq!(rep.demoted[0].run_id, run_id.to_string());
        let row = &list_results(&conn, &r3.id).unwrap()[0];
        assert_eq!(row.result, ScenarioResult::NeedsVerification);
        assert_eq!(row.evidence_ref.as_deref(), Some(run_ref.as_str()));
        assert!(row.note.as_deref().unwrap().contains("stale"));
        assert!(row.note.as_deref().unwrap().contains("green"));
        // …and it is back in the needs-verification queue.
        let needs = scenarios_needing_verification(&conn, &r3.id, &plan_id).unwrap();
        assert_eq!(needs.len(), 1);
        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn carry_over_keeps_free_text_ref_and_note() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Completed);
        insert(&conn, &r1).unwrap();
        upsert_result(
            &conn,
            &r1.id,
            &scn_id,
            ScenarioResult::Failing,
            Some("n"),
            Some("log.txt"),
        )
        .unwrap();
        let r2 = mk_round(plan_id.clone(), 2, RoundStatus::Active);
        insert(&conn, &r2).unwrap();
        carry_over_results(&conn, &r2, &r1.id).unwrap();
        let row = &list_results(&conn, &r2.id).unwrap()[0];
        assert_eq!(row.result, ScenarioResult::Failing);
        assert_eq!(row.evidence_ref.as_deref(), Some("log.txt"));
        assert_eq!(row.note.as_deref(), Some("n"));
    }

    #[test]
    fn active_round_baseline_blob_is_locked() {
        let (pool, plan_id, _scn) = fixture();
        let conn = pool.get().unwrap();
        let planning = mk_round(plan_id.clone(), 1, RoundStatus::Planning);
        insert(&conn, &planning).unwrap();
        set_baseline(&conn, &planning.id, "{\"a\":1}", None).unwrap();
        set_baseline(&conn, &planning.id, "{\"a\":2}", None).unwrap();
        set_status(&conn, &planning.id, RoundStatus::Active, Some(now()), None).unwrap();
        let err = set_baseline(&conn, &planning.id, "{\"a\":3}", None).unwrap_err();
        assert!(matches!(err, DomainError::BaselineLocked(_)), "{err:?}");
        assert_eq!(
            get_baseline(&conn, &planning.id).unwrap().as_deref(),
            Some("{\"a\":2}")
        );
        let ghost = Id::from("DEC-NOPE");
        assert!(set_baseline(&conn, &planning.id, "{}", Some(&ghost)).is_err());
    }

    #[test]
    fn progress_separates_baseline_from_added_scenarios() {
        let (pool, plan_id, scn_id) = fixture();
        let conn = pool.get().unwrap();
        let r1 = mk_round(plan_id.clone(), 1, RoundStatus::Active);
        insert(&conn, &r1).unwrap();
        record_activation_baseline(&conn, &r1, Some("abc")).unwrap();
        // Re-recording the activation baseline is refused.
        assert!(matches!(
            record_activation_baseline(&conn, &r1, Some("def")).unwrap_err(),
            DomainError::BaselineLocked(_)
        ));
        // A scenario added after activation, already passing.
        let added = Scenario {
            id: Id::new(IdKind::Scenario),
            plan_id: plan_id.clone(),
            short_code: format!("SCN-{}", ulid::Ulid::new()),
            given: "g2".into(),
            when_clause: "w2".into(),
            then_clause: "t2".into(),
            tags: vec![],
            origin_round_id: None,
            status: ScenarioStatus::Confirmed,
            depends_on: vec![],
            produced_by: None,
            verified_by: None,
            claimed_resources_json: "[]".into(),
            claim_status: sdi_core::pattern::ClaimStatus::None,
            produced_via_pattern_id: None,
            retired_at: None,
            belongs_to_flow_id: None,
            covers_flow_step: None,
            created_at: now(),
            updated_at: now(),
        };
        scn_repo::insert(&conn, &added).unwrap();
        upsert_result(
            &conn,
            &r1.id,
            &added.id,
            ScenarioResult::Passing,
            None,
            None,
        )
        .unwrap();

        let p = baseline_progress(&conn, &r1).unwrap();
        let base = p.baseline.unwrap();
        assert_eq!(p.baseline_commit.as_deref(), Some("abc"));
        assert_eq!((base.total, base.passing, base.unverified), (1, 0, 1));
        assert_eq!(
            base.percent,
            Some(0.0),
            "added scenario must not inflate baseline %"
        );
        assert_eq!(base.scenario_ids, vec![scn_id.to_string()]);
        assert_eq!(
            (p.added_after_baseline.total, p.added_after_baseline.passing),
            (1, 1)
        );

        upsert_result(&conn, &r1.id, &scn_id, ScenarioResult::Passing, None, None).unwrap();
        let p = baseline_progress(&conn, &r1).unwrap();
        assert_eq!(p.baseline.unwrap().percent, Some(100.0));
    }
}
