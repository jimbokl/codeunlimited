//! Privacy-preserving bounded experiment accounting.

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;

    use crate::types::Request;
    use tempfile::TempDir;

    use super::*;

    fn request(identity: (&'static str, &str, &str), ts: Option<i64>, tokens: [u64; 5]) -> Request {
        let (source, project, session) = identity;
        let [uncached, cache_read, cache_write_5m, cache_write_1h, output] = tokens;
        Request {
            source,
            project: Arc::from(project),
            session: Arc::from(session),
            ts,
            model: Arc::from("private-model"),
            unc_in: uncached,
            cached_in: cache_read,
            w5: cache_write_5m,
            w1h: cache_write_1h,
            out: output,
        }
    }

    #[test]
    fn names_accept_only_bounded_ascii_safe_characters() {
        for valid in ["a", "control-1.8_run"] {
            assert!(validate_name(valid).is_ok(), "expected valid name: {valid}");
        }
        for invalid in [
            "",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "with space",
            "with/slash",
            "unicodé",
        ] {
            assert!(
                validate_name(invalid).is_err(),
                "expected invalid name: {invalid}"
            );
        }
    }

    #[test]
    fn windows_require_a_strictly_increasing_interval() {
        assert!(validate_window(100, 101).is_ok());
        assert!(validate_window(100, 100).is_err());
        assert!(validate_window(101, 100).is_err());
    }

    #[test]
    fn aggregate_uses_half_open_window_and_marks_missing_timestamps() {
        let rows = vec![
            request(("claude", "p", "s"), Some(100), [10, 20, 30, 40, 5]),
            request(("claude", "p", "s"), Some(199), [1, 2, 3, 4, 5]),
            request(("claude", "p", "s"), Some(200), [900, 0, 0, 0, 0]),
            request(("claude", "p", "unknown"), None, [7, 0, 0, 0, 0]),
        ];

        let got = aggregate(&rows, 100, 200);

        assert_eq!(got.totals.requests, 2);
        assert_eq!(got.totals.sessions, 1);
        assert_eq!(got.totals.uncached_input_tokens, 11);
        assert_eq!(got.totals.cache_read_input_tokens, 22);
        assert_eq!(got.totals.cache_write_5m_input_tokens, 33);
        assert_eq!(got.totals.cache_write_1h_input_tokens, 44);
        assert_eq!(got.totals.input_tokens, 110);
        assert_eq!(got.totals.output_tokens, 10);
        assert_eq!(got.totals.total_tokens, 120);
        assert_eq!(got.records_without_timestamp, 1);
        assert!(!got.complete_accounting);
    }

    #[test]
    fn aggregate_counts_composite_source_project_session_identity() {
        let rows = vec![
            request(("claude", "one", "same"), Some(100), [1, 0, 0, 0, 0]),
            request(("claude", "two", "same"), Some(100), [1, 0, 0, 0, 0]),
            request(("codex", "one", "same"), Some(100), [1, 0, 0, 0, 0]),
            request(("claude", "one", "same"), Some(101), [1, 0, 0, 0, 0]),
        ];

        let got = aggregate(&rows, 100, 200);

        assert_eq!(got.totals.requests, 4);
        assert_eq!(got.totals.sessions, 3);
    }

    #[test]
    fn aggregate_saturates_every_derived_total() {
        let rows = vec![
            request(
                ("codex", "p", "s1"),
                Some(100),
                [u64::MAX, 0, 0, 0, u64::MAX],
            ),
            request(("codex", "p", "s2"), Some(101), [1, u64::MAX, 1, 1, 1]),
        ];

        let got = aggregate(&rows, 100, 200);

        assert_eq!(got.totals.requests, 2);
        assert_eq!(got.totals.sessions, 2);
        assert_eq!(got.totals.uncached_input_tokens, u64::MAX);
        assert_eq!(got.totals.cache_read_input_tokens, u64::MAX);
        assert_eq!(got.totals.cache_write_5m_input_tokens, 1);
        assert_eq!(got.totals.cache_write_1h_input_tokens, 1);
        assert_eq!(got.totals.input_tokens, u64::MAX);
        assert_eq!(got.totals.output_tokens, u64::MAX);
        assert_eq!(got.totals.total_tokens, u64::MAX);
    }

    fn completed_record(
        name: &str,
        started_unix: i64,
        finished_unix: i64,
        completed_tasks: u64,
        input_tokens: u64,
    ) -> ExperimentRecord {
        ExperimentRecord {
            name: name.to_string(),
            started_unix,
            finished_unix: Some(finished_unix),
            completed_tasks: Some(completed_tasks),
            status: ExperimentStatus::Complete,
            complete_accounting: true,
            records_without_timestamp: 0,
            totals: Some(TokenTotals {
                requests: 1,
                sessions: 1,
                uncached_input_tokens: input_tokens,
                input_tokens,
                total_tokens: input_tokens,
                ..TokenTotals::default()
            }),
        }
    }

    #[test]
    fn compare_preserves_exact_records_for_one_task() {
        let control = completed_record("control", 100, 200, 1, 100);
        let treatment = completed_record("treatment", 200, 300, 1, 80);

        let got = compare_records(&control, &treatment).unwrap();

        assert_eq!(got.control, control);
        assert_eq!(got.treatment, treatment);
        assert_eq!(got.control_input_tokens_per_task, 100.0);
        assert_eq!(got.treatment_input_tokens_per_task, 80.0);
        assert_eq!(got.observed_input_delta_per_task, -20.0);
        assert_eq!(got.observed_input_change_percent, -20.0);
        assert_eq!(got.observed_capacity_change_percent, 25.0);
        assert_eq!(got.confidence, "low");
        assert_eq!(got.causality, "observational");
    }

    #[test]
    fn compare_uses_task_denominators_for_presentation_values() {
        let control = completed_record("control", 100, 200, 3, 300);
        let treatment = completed_record("treatment", 200, 300, 4, 200);

        let got = compare_records(&control, &treatment).unwrap();

        assert!((got.control_input_tokens_per_task - 100.0).abs() < 1e-9);
        assert!((got.treatment_input_tokens_per_task - 50.0).abs() < 1e-9);
        assert!((got.observed_input_delta_per_task - -50.0).abs() < 1e-9);
        assert!((got.observed_input_change_percent - -50.0).abs() < 1e-9);
        assert!((got.observed_capacity_change_percent - 100.0).abs() < 1e-9);
        assert_eq!(got.confidence, "high");
    }

    #[test]
    fn compare_refuses_active_incomplete_empty_overlapping_and_zero_task_records() {
        let valid_control = completed_record("control", 100, 200, 1, 100);
        let valid_treatment = completed_record("treatment", 200, 300, 1, 80);

        let mut active = valid_control.clone();
        active.status = ExperimentStatus::Active;
        active.finished_unix = None;
        active.completed_tasks = None;
        active.totals = None;
        assert!(compare_records(&active, &valid_treatment).is_err());

        let mut incomplete = valid_control.clone();
        incomplete.complete_accounting = false;
        incomplete.records_without_timestamp = 1;
        assert!(compare_records(&incomplete, &valid_treatment).is_err());

        let mut empty = valid_control.clone();
        empty.totals = Some(TokenTotals::default());
        assert!(compare_records(&empty, &valid_treatment).is_err());

        let overlapping = completed_record("treatment", 199, 300, 1, 80);
        assert!(compare_records(&valid_control, &overlapping).is_err());

        let mut zero_task = valid_control.clone();
        zero_task.completed_tasks = Some(0);
        assert!(compare_records(&zero_task, &valid_treatment).is_err());
    }

    #[test]
    fn state_missing_file_loads_empty_and_valid_state_round_trips() {
        let project = TempDir::new().unwrap();
        assert_eq!(
            load_store(project.path()).unwrap(),
            ExperimentStore::default()
        );

        let mut store = ExperimentStore::default();
        let record = completed_record("control", 100, 200, 1, 100);
        store.records.insert(record.name.clone(), record);

        save_store(project.path(), &store).unwrap();

        assert_eq!(load_store(project.path()).unwrap(), store);
    }

    #[test]
    fn invalid_state_fails_without_changing_original_bytes() {
        let project = TempDir::new().unwrap();
        let path = project.path().join(EXPERIMENT_FILE);

        for invalid in [
            br#"{"schema_version":2,"records":{}}"#.as_slice(),
            br#"{"schema_version":1,"records":"not-a-map"}"#.as_slice(),
            br#"{"schema_version":1,"records":{"same":{"name":"same","started_unix":1,"finished_unix":2,"completed_tasks":1,"status":"complete","complete_accounting":true,"records_without_timestamp":0,"totals":{"requests":1,"sessions":1,"uncached_input_tokens":1,"cache_read_input_tokens":0,"cache_write_5m_input_tokens":0,"cache_write_1h_input_tokens":0,"input_tokens":1,"output_tokens":0,"total_tokens":1}},"same":{"name":"same","started_unix":1,"finished_unix":2,"completed_tasks":1,"status":"complete","complete_accounting":true,"records_without_timestamp":0,"totals":{"requests":1,"sessions":1,"uncached_input_tokens":1,"cache_read_input_tokens":0,"cache_write_5m_input_tokens":0,"cache_write_1h_input_tokens":0,"input_tokens":1,"output_tokens":0,"total_tokens":1}}}}"#.as_slice(),
            br#"not-json"#.as_slice(),
        ] {
            fs::write(&path, invalid).unwrap();
            let before = fs::read(&path).unwrap();

            assert!(load_store(project.path()).is_err());
            assert_eq!(fs::read(&path).unwrap(), before);
        }
    }
}
use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use fs2::FileExt;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::config::Config;
use crate::parsers;
use crate::safeio::{atomic_write, read_optional_text, reject_symlink};
use crate::types::Request;

pub const EXPERIMENT_FILE: &str = ".codeunlimited.experiments.json";
pub const EXPERIMENT_LOCK_FILE: &str = ".codeunlimited.experiments.lock";
pub const SCHEMA_VERSION: u64 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenTotals {
    pub requests: u64,
    pub sessions: u64,
    pub uncached_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_write_5m_input_tokens: u64,
    pub cache_write_1h_input_tokens: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExperimentStatus {
    Active,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentRecord {
    pub name: String,
    pub started_unix: i64,
    pub finished_unix: Option<i64>,
    pub completed_tasks: Option<u64>,
    pub status: ExperimentStatus,
    pub complete_accounting: bool,
    pub records_without_timestamp: u64,
    pub totals: Option<TokenTotals>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentStore {
    pub schema_version: u64,
    #[serde(deserialize_with = "deserialize_unique_records")]
    pub records: BTreeMap<String, ExperimentRecord>,
}

fn deserialize_unique_records<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<String, ExperimentRecord>, D::Error>
where
    D: Deserializer<'de>,
{
    struct UniqueRecords;

    impl<'de> Visitor<'de> for UniqueRecords {
        type Value = BTreeMap<String, ExperimentRecord>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a map with unique experiment names")
        }

        fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            let mut records = BTreeMap::new();
            while let Some((name, record)) = map.next_entry::<String, ExperimentRecord>()? {
                if records.insert(name.clone(), record).is_some() {
                    return Err(de::Error::custom(format!(
                        "duplicate experiment name: {name}"
                    )));
                }
            }
            Ok(records)
        }
    }

    deserializer.deserialize_map(UniqueRecords)
}

impl Default for ExperimentStore {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            records: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Accounting {
    pub complete_accounting: bool,
    pub records_without_timestamp: u64,
    pub totals: TokenTotals,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Comparison {
    pub control: ExperimentRecord,
    pub treatment: ExperimentRecord,
    pub control_input_tokens_per_task: f64,
    pub treatment_input_tokens_per_task: f64,
    pub observed_input_delta_per_task: f64,
    pub observed_input_change_percent: f64,
    pub observed_capacity_change_percent: f64,
    pub confidence: String,
    pub causality: String,
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 64 {
        return Err("experiment name must be between 1 and 64 bytes".to_string());
    }
    if !name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(
            "experiment name may contain only ASCII letters, digits, '.', '_', or '-'".to_string(),
        );
    }
    Ok(())
}

pub fn validate_window(started_unix: i64, finished_unix: i64) -> Result<(), String> {
    if started_unix >= finished_unix {
        return Err("experiment start must be before finish".to_string());
    }
    Ok(())
}

pub fn aggregate(requests: &[Request], started_unix: i64, finished_unix: i64) -> Accounting {
    let records_without_timestamp = requests
        .iter()
        .filter(|request| request.ts.is_none())
        .fold(0_u64, |count, _| count.saturating_add(1));
    let mut sessions = HashSet::new();
    let mut totals = TokenTotals::default();

    for request in requests.iter().filter(|request| {
        request
            .ts
            .is_some_and(|timestamp| started_unix <= timestamp && timestamp < finished_unix)
    }) {
        totals.requests = totals.requests.saturating_add(1);
        totals.uncached_input_tokens = totals.uncached_input_tokens.saturating_add(request.unc_in);
        totals.cache_read_input_tokens = totals
            .cache_read_input_tokens
            .saturating_add(request.cached_in);
        totals.cache_write_5m_input_tokens = totals
            .cache_write_5m_input_tokens
            .saturating_add(request.w5);
        totals.cache_write_1h_input_tokens = totals
            .cache_write_1h_input_tokens
            .saturating_add(request.w1h);
        totals.output_tokens = totals.output_tokens.saturating_add(request.out);
        sessions.insert((
            request.source,
            request.project.as_ref(),
            request.session.as_ref(),
        ));
    }

    totals.sessions = u64::try_from(sessions.len()).unwrap_or(u64::MAX);
    totals.input_tokens = totals
        .uncached_input_tokens
        .saturating_add(totals.cache_read_input_tokens)
        .saturating_add(totals.cache_write_5m_input_tokens)
        .saturating_add(totals.cache_write_1h_input_tokens);
    totals.total_tokens = totals.input_tokens.saturating_add(totals.output_tokens);

    Accounting {
        complete_accounting: records_without_timestamp == 0,
        records_without_timestamp,
        totals,
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn comparison_parts(record: &ExperimentRecord) -> Result<(i64, u64, &TokenTotals), String> {
    if record.status != ExperimentStatus::Complete {
        return Err(format!("experiment '{}' is still active", record.name));
    }
    if !record.complete_accounting || record.records_without_timestamp != 0 {
        return Err(format!(
            "experiment '{}' has incomplete timestamp accounting",
            record.name
        ));
    }
    let finished_unix = record
        .finished_unix
        .ok_or_else(|| format!("experiment '{}' has no finish time", record.name))?;
    let completed_tasks = record
        .completed_tasks
        .filter(|tasks| *tasks > 0)
        .ok_or_else(|| format!("experiment '{}' has no completed tasks", record.name))?;
    let totals = record
        .totals
        .as_ref()
        .ok_or_else(|| format!("experiment '{}' has no token totals", record.name))?;
    if totals.requests == 0 {
        return Err(format!("experiment '{}' contains no requests", record.name));
    }
    if totals.input_tokens == 0 {
        return Err(format!(
            "experiment '{}' contains zero input tokens",
            record.name
        ));
    }
    Ok((finished_unix, completed_tasks, totals))
}

pub fn compare_records(
    control: &ExperimentRecord,
    treatment: &ExperimentRecord,
) -> Result<Comparison, String> {
    let (control_finished, control_tasks, control_totals) = comparison_parts(control)?;
    let (treatment_finished, treatment_tasks, treatment_totals) = comparison_parts(treatment)?;
    if control.started_unix < treatment_finished && treatment.started_unix < control_finished {
        return Err("experiment windows overlap".to_string());
    }

    let control_per_task = control_totals.input_tokens as f64 / control_tasks as f64;
    let treatment_per_task = treatment_totals.input_tokens as f64 / treatment_tasks as f64;
    let delta_per_task = treatment_per_task - control_per_task;

    Ok(Comparison {
        control: control.clone(),
        treatment: treatment.clone(),
        control_input_tokens_per_task: control_per_task,
        treatment_input_tokens_per_task: treatment_per_task,
        observed_input_delta_per_task: delta_per_task,
        observed_input_change_percent: 100.0 * delta_per_task / control_per_task,
        observed_capacity_change_percent: 100.0 * (control_per_task / treatment_per_task - 1.0),
        confidence: if control_tasks < 3 || treatment_tasks < 3 {
            "low".to_string()
        } else {
            "high".to_string()
        },
        causality: "observational".to_string(),
    })
}

fn experiment_path(project: &Path) -> PathBuf {
    project.join(EXPERIMENT_FILE)
}

fn lock_store(project: &Path) -> io::Result<File> {
    let path = project.join(EXPERIMENT_LOCK_FILE);
    reject_symlink(&path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    FileExt::lock_exclusive(&file)?;
    Ok(file)
}

fn validate_totals(totals: &TokenTotals) -> io::Result<()> {
    if totals.sessions > totals.requests {
        return Err(invalid_data("experiment sessions exceed requests"));
    }
    let input_tokens = totals
        .uncached_input_tokens
        .saturating_add(totals.cache_read_input_tokens)
        .saturating_add(totals.cache_write_5m_input_tokens)
        .saturating_add(totals.cache_write_1h_input_tokens);
    if totals.input_tokens != input_tokens {
        return Err(invalid_data("experiment input total is inconsistent"));
    }
    if totals.total_tokens != totals.input_tokens.saturating_add(totals.output_tokens) {
        return Err(invalid_data("experiment total token count is inconsistent"));
    }
    Ok(())
}

fn validate_record(key: &str, record: &ExperimentRecord) -> io::Result<()> {
    validate_name(key).map_err(invalid_data)?;
    if record.name != key {
        return Err(invalid_data(
            "experiment record name does not match its key",
        ));
    }
    match record.status {
        ExperimentStatus::Active => {
            if record.finished_unix.is_some()
                || record.completed_tasks.is_some()
                || record.totals.is_some()
                || record.complete_accounting
                || record.records_without_timestamp != 0
            {
                return Err(invalid_data("active experiment contains completed data"));
            }
        }
        ExperimentStatus::Complete => {
            if record.complete_accounting != (record.records_without_timestamp == 0) {
                return Err(invalid_data(
                    "experiment timestamp accounting fields are inconsistent",
                ));
            }
            let finished_unix = record
                .finished_unix
                .ok_or_else(|| invalid_data("completed experiment has no finish time"))?;
            validate_window(record.started_unix, finished_unix).map_err(invalid_data)?;
            if record.completed_tasks.filter(|tasks| *tasks > 0).is_none() {
                return Err(invalid_data(
                    "completed experiment must declare at least one task",
                ));
            }
            validate_totals(
                record
                    .totals
                    .as_ref()
                    .ok_or_else(|| invalid_data("completed experiment has no totals"))?,
            )?;
        }
    }
    Ok(())
}

fn validate_store(store: &ExperimentStore) -> io::Result<()> {
    if store.schema_version != SCHEMA_VERSION {
        return Err(invalid_data(format!(
            "unsupported experiment schema version {}",
            store.schema_version
        )));
    }
    for (key, record) in &store.records {
        validate_record(key, record)?;
    }
    Ok(())
}

pub fn load_store(project: &Path) -> io::Result<ExperimentStore> {
    let path = experiment_path(project);
    let Some(text) = read_optional_text(&path)? else {
        return Ok(ExperimentStore::default());
    };
    let store: ExperimentStore =
        serde_json::from_str(&text).map_err(|error| invalid_data(error.to_string()))?;
    validate_store(&store)?;
    Ok(store)
}

pub fn save_store(project: &Path, store: &ExperimentStore) -> io::Result<()> {
    validate_store(store)?;
    let mut bytes =
        serde_json::to_vec_pretty(store).map_err(|error| invalid_data(error.to_string()))?;
    bytes.push(b'\n');
    atomic_write(&experiment_path(project), &bytes)
}

fn positive_tasks(completed_tasks: u64) -> Result<(), String> {
    if completed_tasks == 0 {
        return Err("completed tasks must be at least 1".to_string());
    }
    Ok(())
}

fn parse_boundary(value: &str, label: &str) -> Result<i64, String> {
    let timestamp = DateTime::parse_from_rfc3339(value)
        .map_err(|_| format!("invalid RFC 3339 {label} timestamp: {value}"))?;
    if timestamp.timestamp_subsec_nanos() != 0 {
        return Err(format!(
            "{label} timestamp must use whole-second precision: {value}"
        ));
    }
    Ok(timestamp.timestamp())
}

fn scan_project(project: &Path) -> io::Result<Vec<Request>> {
    let mut requests = parsers::iter_claude_checked(Some(project))?;
    requests.extend(parsers::iter_codex_checked(Some(project))?);
    let config = Config::load_for(Some(project));
    requests.retain(|request| !config.is_ignored(&request.project));
    Ok(requests)
}

fn completed_record_from_scan(
    name: &str,
    started_unix: i64,
    finished_unix: i64,
    completed_tasks: u64,
    requests: &[Request],
) -> ExperimentRecord {
    let accounting = aggregate(requests, started_unix, finished_unix);
    ExperimentRecord {
        name: name.to_string(),
        started_unix,
        finished_unix: Some(finished_unix),
        completed_tasks: Some(completed_tasks),
        status: ExperimentStatus::Complete,
        complete_accounting: accounting.complete_accounting,
        records_without_timestamp: accounting.records_without_timestamp,
        totals: Some(accounting.totals),
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
    let output = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    println!("{output}");
    Ok(())
}

fn print_record(record: &ExperimentRecord, json: bool) -> Result<(), String> {
    if json {
        return print_json(record);
    }
    match (&record.status, &record.totals) {
        (ExperimentStatus::Active, _) => {
            println!(
                "experiment '{}' started at Unix {}",
                record.name, record.started_unix
            );
        }
        (ExperimentStatus::Complete, Some(totals)) => {
            println!(
                "experiment '{}' exact observed counters: {} requests, {} input tokens, {} output tokens, {} total tokens",
                record.name,
                totals.requests,
                totals.input_tokens,
                totals.output_tokens,
                totals.total_tokens
            );
            if !record.complete_accounting {
                println!(
                    "incomplete accounting: {} recognized records had no timestamp",
                    record.records_without_timestamp
                );
            }
        }
        (ExperimentStatus::Complete, None) => {
            return Err("completed experiment has no totals".to_string());
        }
    }
    Ok(())
}

fn command_result(run: impl FnOnce() -> Result<(), String>) -> i32 {
    match run() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("error: {error}");
            1
        }
    }
}

pub fn start(name: &str, project: &Path) -> i32 {
    command_result(|| {
        validate_name(name)?;
        let _lock = lock_store(project).map_err(|error| error.to_string())?;
        let started_unix = Utc::now().timestamp();
        let mut store = load_store(project).map_err(|error| error.to_string())?;
        if store.records.contains_key(name) {
            return Err(format!("experiment '{name}' already exists"));
        }
        let record = ExperimentRecord {
            name: name.to_string(),
            started_unix,
            finished_unix: None,
            completed_tasks: None,
            status: ExperimentStatus::Active,
            complete_accounting: false,
            records_without_timestamp: 0,
            totals: None,
        };
        store.records.insert(name.to_string(), record.clone());
        save_store(project, &store).map_err(|error| error.to_string())?;
        print_record(&record, false)
    })
}

pub fn finish(name: &str, completed_tasks: u64, project: &Path, json: bool) -> i32 {
    command_result(|| {
        validate_name(name)?;
        positive_tasks(completed_tasks)?;
        let _lock = lock_store(project).map_err(|error| error.to_string())?;
        let finished_unix = Utc::now().timestamp();
        let mut store = load_store(project).map_err(|error| error.to_string())?;
        let existing = store
            .records
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown experiment '{name}'"))?;
        if existing.status == ExperimentStatus::Complete {
            return print_record(&existing, json);
        }
        validate_window(existing.started_unix, finished_unix)?;
        let requests = scan_project(project)
            .map_err(|error| format!("cannot scan experiment logs: {error}"))?;
        let record = completed_record_from_scan(
            name,
            existing.started_unix,
            finished_unix,
            completed_tasks,
            &requests,
        );
        store.records.insert(name.to_string(), record.clone());
        save_store(project, &store).map_err(|error| error.to_string())?;
        print_record(&record, json)
    })
}

pub fn record(
    name: &str,
    from: &str,
    to: &str,
    completed_tasks: u64,
    project: &Path,
    json: bool,
) -> i32 {
    command_result(|| {
        validate_name(name)?;
        positive_tasks(completed_tasks)?;
        let started_unix = parse_boundary(from, "start")?;
        let finished_unix = parse_boundary(to, "finish")?;
        validate_window(started_unix, finished_unix)?;
        let _lock = lock_store(project).map_err(|error| error.to_string())?;
        let mut store = load_store(project).map_err(|error| error.to_string())?;
        if store.records.contains_key(name) {
            return Err(format!("experiment '{name}' already exists"));
        }
        let requests = scan_project(project)
            .map_err(|error| format!("cannot scan experiment logs: {error}"))?;
        let record = completed_record_from_scan(
            name,
            started_unix,
            finished_unix,
            completed_tasks,
            &requests,
        );
        store.records.insert(name.to_string(), record.clone());
        save_store(project, &store).map_err(|error| error.to_string())?;
        print_record(&record, json)
    })
}

pub fn compare(control: &str, treatment: &str, project: &Path, json: bool) -> i32 {
    command_result(|| {
        let store = load_store(project).map_err(|error| error.to_string())?;
        let control_record = store
            .records
            .get(control)
            .ok_or_else(|| format!("unknown experiment '{control}'"))?;
        let treatment_record = store
            .records
            .get(treatment)
            .ok_or_else(|| format!("unknown experiment '{treatment}'"))?;
        let comparison = compare_records(control_record, treatment_record)?;
        if json {
            print_json(&comparison)
        } else {
            let control_totals = comparison
                .control
                .totals
                .as_ref()
                .ok_or_else(|| "control experiment has no totals".to_string())?;
            let treatment_totals = comparison
                .treatment
                .totals
                .as_ref()
                .ok_or_else(|| "treatment experiment has no totals".to_string())?;
            println!(
                "exact observed counters: control={} input tokens/tasks={}; treatment={} input tokens/tasks={}",
                control_totals.input_tokens,
                comparison.control.completed_tasks.unwrap_or(0),
                treatment_totals.input_tokens,
                comparison.treatment.completed_tasks.unwrap_or(0)
            );
            if comparison.observed_input_delta_per_task < 0.0 {
                println!(
                    "treatment has {:.0} lower observed input tokens per task ({:.1}% change); observed capacity change {:+.1}%",
                    comparison.observed_input_delta_per_task.abs(),
                    comparison.observed_input_change_percent,
                    comparison.observed_capacity_change_percent
                );
            } else if comparison.observed_input_delta_per_task > 0.0 {
                println!(
                    "treatment has {:.0} higher observed input tokens per task ({:+.1}% change); observed capacity change {:+.1}%",
                    comparison.observed_input_delta_per_task,
                    comparison.observed_input_change_percent,
                    comparison.observed_capacity_change_percent
                );
            } else {
                println!("treatment has equal observed input tokens per task (0.0% change)");
            }
            if comparison.confidence == "low" {
                println!("low confidence: either arm has fewer than three completed tasks");
            }
            println!(
                "observational comparison: this observed difference does not establish causality"
            );
            Ok(())
        }
    })
}

pub fn list(project: &Path, json: bool) -> i32 {
    command_result(|| {
        let store = load_store(project).map_err(|error| error.to_string())?;
        if json {
            let records: Vec<&ExperimentRecord> = store.records.values().collect();
            return print_json(&records);
        }
        for record in store.records.values() {
            let finished = record
                .finished_unix
                .map_or_else(|| "active".to_string(), |value| value.to_string());
            let tasks = record
                .completed_tasks
                .map_or_else(|| "-".to_string(), |value| value.to_string());
            let totals = record.totals.as_ref().map_or_else(
                || "-".to_string(),
                |totals| {
                    format!(
                        "{} input / {} output / {} total",
                        totals.input_tokens, totals.output_tokens, totals.total_tokens
                    )
                },
            );
            println!(
                "{} {:?} {}..{} tasks={} {}",
                record.name, record.status, record.started_unix, finished, tasks, totals
            );
        }
        Ok(())
    })
}
