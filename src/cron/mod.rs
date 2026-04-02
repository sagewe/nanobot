//! Cron service — schedule recurring or one-shot agent tasks.
//!
//! Mirrors the Python `nanobot.cron` module.  Jobs are persisted as JSON at
//! `<workspace>/cron/jobs.json` and hot-reloaded whenever the file is modified
//! externally (same behaviour as the Python version).

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tracing::{info, warn};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public types (JSON-compatible with the Python implementation)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ScheduleKind {
    At,
    Every,
    Cron,
}

/// Describes when a job should run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronSchedule {
    pub kind: ScheduleKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expr: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tz: Option<String>,
}

impl CronSchedule {
    pub fn at(at_ms: i64) -> Self {
        Self {
            kind: ScheduleKind::At,
            at_ms: Some(at_ms),
            every_ms: None,
            expr: None,
            tz: None,
        }
    }

    pub fn every(every_ms: i64) -> Self {
        Self {
            kind: ScheduleKind::Every,
            at_ms: None,
            every_ms: Some(every_ms),
            expr: None,
            tz: None,
        }
    }

    pub fn cron(expr: impl Into<String>, tz: Option<String>) -> Self {
        Self {
            kind: ScheduleKind::Cron,
            at_ms: None,
            every_ms: None,
            expr: Some(expr.into()),
            tz,
        }
    }
}

/// What to do when the job fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronPayload {
    #[serde(default = "default_payload_kind")]
    pub kind: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub deliver: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
}

fn default_payload_kind() -> String {
    "agent_turn".to_string()
}

impl Default for CronPayload {
    fn default() -> Self {
        Self {
            kind: default_payload_kind(),
            message: String::new(),
            deliver: false,
            channel: None,
            to: None,
        }
    }
}

/// Runtime state persisted alongside the job definition.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJobState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// A complete scheduled job entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJob {
    pub id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub schedule: CronSchedule,
    #[serde(default)]
    pub payload: CronPayload,
    #[serde(default)]
    pub state: CronJobState,
    #[serde(default)]
    pub created_at_ms: i64,
    #[serde(default)]
    pub updated_at_ms: i64,
    #[serde(default)]
    pub delete_after_run: bool,
}

fn default_true() -> bool {
    true
}

/// Top-level store written to disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronStore {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub jobs: Vec<CronJob>,
}

fn default_version() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Schedule computation
// ---------------------------------------------------------------------------

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Compute the next run timestamp (ms since epoch) for a schedule, given the
/// current time.  Returns `None` when the schedule produces no future run.
fn compute_next_run(schedule: &CronSchedule, now_ms: i64) -> Option<i64> {
    match schedule.kind {
        ScheduleKind::At => schedule.at_ms.filter(|&t| t > now_ms),
        ScheduleKind::Every => {
            let every = schedule.every_ms.filter(|&e| e > 0)?;
            Some(now_ms + every)
        }
        ScheduleKind::Cron => {
            let expr = schedule.expr.as_deref()?;
            compute_next_cron(expr, schedule.tz.as_deref(), now_ms)
                .map_err(|e| warn!("Cron: bad expression '{}': {}", expr, e))
                .ok()
        }
    }
}

fn compute_next_cron(expr: &str, tz: Option<&str>, base_ms: i64) -> Result<i64> {
    use croner::Cron;

    let cron = Cron::new(expr)
        .with_seconds_optional()
        .parse()
        .map_err(|e| anyhow!("invalid cron expression '{}': {}", expr, e))?;

    if let Some(tz_str) = tz {
        let tz: chrono_tz::Tz = tz_str
            .parse()
            .with_context(|| format!("unknown timezone '{tz_str}'"))?;
        let base = DateTime::from_timestamp_millis(base_ms)
            .unwrap_or_else(|| Utc::now())
            .with_timezone(&tz);
        let next = cron
            .find_next_occurrence(&base, false)
            .map_err(|e| anyhow!("no next occurrence: {}", e))?;
        Ok(next.with_timezone(&Utc).timestamp_millis())
    } else {
        use chrono::Local;
        let base = DateTime::from_timestamp_millis(base_ms)
            .unwrap_or_else(|| Utc::now())
            .with_timezone(&Local);
        let next = cron
            .find_next_occurrence(&base, false)
            .map_err(|e| anyhow!("no next occurrence: {}", e))?;
        Ok(next.with_timezone(&Utc).timestamp_millis())
    }
}

/// Validate schedule before adding a job (mirrors Python's
/// `_validate_schedule_for_add`).
fn validate_schedule(schedule: &CronSchedule) -> Result<()> {
    if schedule.tz.is_some() && schedule.kind != ScheduleKind::Cron {
        anyhow::bail!("tz can only be used with cron schedules");
    }
    if schedule.kind == ScheduleKind::Cron {
        if let Some(tz_str) = &schedule.tz {
            let _: chrono_tz::Tz = tz_str
                .parse()
                .with_context(|| format!("unknown timezone '{tz_str}'"))?;
        }
        let expr = schedule.expr.as_deref().unwrap_or("");
        croner::Cron::new(expr)
            .with_seconds_optional()
            .parse()
            .map_err(|e| anyhow!("invalid cron expression '{}': {}", expr, e))?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CronService
// ---------------------------------------------------------------------------

type JobCallback =
    Arc<dyn Fn(CronJob) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

struct CronInner {
    store: CronStore,
    last_mtime: Option<SystemTime>,
}

/// Manages scheduled jobs: loads/saves from disk, fires callbacks on schedule.
pub struct CronService {
    store_path: PathBuf,
    on_job: Mutex<Option<JobCallback>>,
    inner: Arc<Mutex<CronInner>>,
    notify: Arc<Notify>,
    running: Arc<AtomicBool>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl CronService {
    pub fn new(store_path: PathBuf) -> Self {
        Self {
            store_path,
            on_job: Mutex::new(None),
            inner: Arc::new(Mutex::new(CronInner {
                store: CronStore::default(),
                last_mtime: None,
            })),
            notify: Arc::new(Notify::new()),
            running: Arc::new(AtomicBool::new(false)),
            task: Mutex::new(None),
        }
    }

    /// Set the callback that runs every time a job fires.
    pub fn set_on_job<F, Fut>(&self, f: F)
    where
        F: Fn(CronJob) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Option<String>> + Send + 'static,
    {
        *self.on_job.lock().unwrap() = Some(Arc::new(move |job| Box::pin(f(job))));
    }

    /// Start the background timer loop.
    pub async fn start(self: &Arc<Self>) {
        if self.running.swap(true, Ordering::SeqCst) {
            return; // already running
        }
        {
            let mut inner = self.inner.lock().unwrap();
            self.load_store_locked(&mut inner);
            recompute_next_runs_locked(&mut inner, now_ms());
            self.save_store_locked(&inner);
        }
        let job_count = self.inner.lock().unwrap().store.jobs.len();
        info!("Cron service started with {} jobs", job_count);

        let svc = Arc::clone(self);
        let handle = tokio::spawn(async move {
            svc.run_loop().await;
        });
        *self.task.lock().unwrap() = Some(handle);
    }

    /// Stop the background timer loop.
    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.notify.notify_waiters();
        if let Some(task) = self.task.lock().unwrap().take() {
            task.abort();
        }
    }

    // -----------------------------------------------------------------------
    // Public API
    // -----------------------------------------------------------------------

    /// List jobs.  When `include_disabled` is false only enabled jobs are
    /// returned, sorted by next run time ascending.
    pub fn list_jobs(&self, include_disabled: bool) -> Vec<CronJob> {
        let mut inner = self.inner.lock().unwrap();
        self.load_store_locked(&mut inner);
        let mut jobs: Vec<CronJob> = inner
            .store
            .jobs
            .iter()
            .filter(|j| include_disabled || j.enabled)
            .cloned()
            .collect();
        jobs.sort_by_key(|j| j.state.next_run_at_ms.unwrap_or(i64::MAX));
        jobs
    }

    /// Add a new job and persist immediately.
    pub fn add_job(
        &self,
        name: impl Into<String>,
        schedule: CronSchedule,
        message: impl Into<String>,
        deliver: bool,
        channel: Option<String>,
        to: Option<String>,
        delete_after_run: bool,
    ) -> Result<CronJob> {
        validate_schedule(&schedule)?;
        let now = now_ms();
        let job = CronJob {
            id: Uuid::new_v4().to_string()[..8].to_string(),
            name: name.into(),
            enabled: true,
            state: CronJobState {
                next_run_at_ms: compute_next_run(&schedule, now),
                ..Default::default()
            },
            schedule,
            payload: CronPayload {
                message: message.into(),
                deliver,
                channel,
                to,
                ..Default::default()
            },
            created_at_ms: now,
            updated_at_ms: now,
            delete_after_run,
        };

        {
            let mut inner = self.inner.lock().unwrap();
            self.load_store_locked(&mut inner);
            inner.store.jobs.push(job.clone());
            self.save_store_locked(&inner);
        }
        self.notify.notify_waiters();
        info!("Cron: added job '{}' ({})", job.name, job.id);
        Ok(job)
    }

    /// Remove a job by ID.  Returns `true` if it existed.
    pub fn remove_job(&self, job_id: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        self.load_store_locked(&mut inner);
        let before = inner.store.jobs.len();
        inner.store.jobs.retain(|j| j.id != job_id);
        let removed = inner.store.jobs.len() < before;
        if removed {
            self.save_store_locked(&inner);
            self.notify.notify_waiters();
            info!("Cron: removed job {}", job_id);
        }
        removed
    }

    /// Enable or disable a job.
    pub fn enable_job(&self, job_id: &str, enabled: bool) -> Option<CronJob> {
        let mut inner = self.inner.lock().unwrap();
        self.load_store_locked(&mut inner);
        let now = now_ms();
        if let Some(job) = inner.store.jobs.iter_mut().find(|j| j.id == job_id) {
            job.enabled = enabled;
            job.updated_at_ms = now;
            job.state.next_run_at_ms = if enabled {
                compute_next_run(&job.schedule, now)
            } else {
                None
            };
            let result = job.clone();
            self.save_store_locked(&inner);
            self.notify.notify_waiters();
            Some(result)
        } else {
            None
        }
    }

    /// Toggle a job's enabled state atomically.  Returns the updated job or
    /// `None` if no job with that ID exists.
    pub fn toggle_job(&self, job_id: &str) -> Option<CronJob> {
        let mut inner = self.inner.lock().unwrap();
        self.load_store_locked(&mut inner);
        let now = now_ms();
        if let Some(job) = inner.store.jobs.iter_mut().find(|j| j.id == job_id) {
            job.enabled = !job.enabled;
            job.updated_at_ms = now;
            job.state.next_run_at_ms = if job.enabled {
                compute_next_run(&job.schedule, now)
            } else {
                None
            };
            let result = job.clone();
            self.save_store_locked(&inner);
            self.notify.notify_waiters();
            Some(result)
        } else {
            None
        }
    }

    /// Manually trigger a job regardless of its schedule.
    pub async fn run_job(self: &Arc<Self>, job_id: &str, force: bool) -> bool {
        let job = {
            let mut inner = self.inner.lock().unwrap();
            self.load_store_locked(&mut inner);
            inner
                .store
                .jobs
                .iter()
                .find(|j| j.id == job_id && (force || j.enabled))
                .cloned()
        };
        let Some(job) = job else {
            return false;
        };
        self.execute_job(job).await;
        {
            let inner = self.inner.lock().unwrap();
            self.save_store_locked(&inner);
        }
        self.notify.notify_waiters();
        true
    }

    /// Summary for logging / status endpoints.
    pub fn status(&self) -> serde_json::Value {
        let inner = self.inner.lock().unwrap();
        serde_json::json!({
            "running": self.running.load(Ordering::SeqCst),
            "jobs": inner.store.jobs.len(),
            "nextWakeAtMs": get_next_wake_ms_locked(&inner),
        })
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn load_store_locked(&self, inner: &mut CronInner) {
        if self.store_path.exists() {
            let mtime = self.store_path.metadata().and_then(|m| m.modified()).ok();
            if mtime != inner.last_mtime {
                info!("Cron: jobs.json modified externally, reloading");
                inner.last_mtime = None; // force reload below
            }
            if inner.last_mtime.is_none() {
                match std::fs::read_to_string(&self.store_path) {
                    Ok(text) => match serde_json::from_str::<CronStore>(&text) {
                        Ok(store) => {
                            inner.store = store;
                            inner.last_mtime = mtime;
                        }
                        Err(e) => warn!("Cron: failed to parse jobs.json: {}", e),
                    },
                    Err(e) => warn!("Cron: failed to read jobs.json: {}", e),
                }
            }
        }
    }

    fn save_store_locked(&self, inner: &CronInner) {
        if let Some(parent) = self.store_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                warn!("Cron: could not create store dir: {}", e);
                return;
            }
        }
        match serde_json::to_string_pretty(&inner.store) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&self.store_path, &text) {
                    warn!("Cron: failed to write jobs.json: {}", e);
                }
            }
            Err(e) => warn!("Cron: failed to serialise store: {}", e),
        }
    }

    async fn run_loop(self: Arc<Self>) {
        while self.running.load(Ordering::SeqCst) {
            // Hot-reload check.
            {
                let mut inner = self.inner.lock().unwrap();
                self.load_store_locked(&mut inner);
            }

            let next_wake = {
                let inner = self.inner.lock().unwrap();
                get_next_wake_ms_locked(&inner)
            };

            if let Some(wake_ms) = next_wake {
                let delay_ms = (wake_ms - now_ms()).max(0) as u64;
                let sleep = tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms));
                tokio::select! {
                    _ = sleep => {
                        self.on_timer().await;
                    }
                    _ = self.notify.notified() => {
                        // store changed or stop requested — re-evaluate
                    }
                }
            } else {
                // No scheduled jobs — wait until notified.
                self.notify.notified().await;
            }
        }
    }

    async fn on_timer(self: &Arc<Self>) {
        let due: Vec<CronJob> = {
            let mut inner = self.inner.lock().unwrap();
            self.load_store_locked(&mut inner);
            let now = now_ms();
            inner
                .store
                .jobs
                .iter()
                .filter(|j| j.enabled && j.state.next_run_at_ms.map_or(false, |t| now >= t))
                .cloned()
                .collect()
        };

        for job in due {
            self.execute_job(job).await;
        }

        let inner = self.inner.lock().unwrap();
        self.save_store_locked(&inner);
    }

    async fn execute_job(self: &Arc<Self>, job: CronJob) {
        let start_ms = now_ms();
        info!("Cron: executing job '{}' ({})", job.name, job.id);

        let cb = self.on_job.lock().unwrap().clone();
        let (status, error_msg): (&str, Option<String>) = if let Some(cb) = cb {
            match cb(job.clone()).await {
                Some(_) => ("ok", None),
                None => ("ok", None),
            }
        } else {
            ("skipped", None)
        };

        if status == "ok" {
            info!("Cron: job '{}' completed", job.name);
        }

        let mut inner = self.inner.lock().unwrap();
        if let Some(j) = inner.store.jobs.iter_mut().find(|j| j.id == job.id) {
            j.state.last_run_at_ms = Some(start_ms);
            j.state.last_status = Some(status.to_string());
            j.state.last_error = error_msg;
            j.updated_at_ms = now_ms();

            match j.schedule.kind {
                ScheduleKind::At => {
                    if j.delete_after_run {
                        let id = j.id.clone();
                        drop(inner);
                        let mut inner = self.inner.lock().unwrap();
                        inner.store.jobs.retain(|jj| jj.id != id);
                        return;
                    } else {
                        j.enabled = false;
                        j.state.next_run_at_ms = None;
                    }
                }
                _ => {
                    let next = compute_next_run(&j.schedule, now_ms());
                    j.state.next_run_at_ms = next;
                }
            }
        }
    }
}

fn recompute_next_runs_locked(inner: &mut CronInner, now: i64) {
    for job in &mut inner.store.jobs {
        if job.enabled {
            job.state.next_run_at_ms = compute_next_run(&job.schedule, now);
        }
    }
}

fn get_next_wake_ms_locked(inner: &CronInner) -> Option<i64> {
    inner
        .store
        .jobs
        .iter()
        .filter(|j| j.enabled)
        .filter_map(|j| j.state.next_run_at_ms)
        .min()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn compute_next_run_every() {
        let sched = CronSchedule::every(60_000);
        let now = 1_000_000;
        assert_eq!(compute_next_run(&sched, now), Some(1_060_000));
    }

    #[test]
    fn compute_next_run_at_future() {
        let sched = CronSchedule::at(2_000_000);
        assert_eq!(compute_next_run(&sched, 1_000_000), Some(2_000_000));
    }

    #[test]
    fn compute_next_run_at_past_returns_none() {
        let sched = CronSchedule::at(500_000);
        assert_eq!(compute_next_run(&sched, 1_000_000), None);
    }

    #[test]
    fn compute_next_run_cron_utc() {
        // "every minute" — next run should be within the next 60s
        let sched = CronSchedule::cron("* * * * *", None);
        let now = now_ms();
        let next = compute_next_run(&sched, now).expect("next");
        assert!(next > now && next <= now + 61_000, "next={next} now={now}");
    }

    #[test]
    fn validate_tz_only_for_cron() {
        let mut sched = CronSchedule::every(1000);
        sched.tz = Some("UTC".to_string());
        assert!(validate_schedule(&sched).is_err());
    }

    #[tokio::test]
    async fn add_remove_job() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("cron").join("jobs.json");
        let svc = Arc::new(CronService::new(path.clone()));

        let job = svc
            .add_job(
                "test",
                CronSchedule::every(60_000),
                "hello",
                false,
                None,
                None,
                false,
            )
            .unwrap();
        assert_eq!(svc.list_jobs(false).len(), 1);

        let removed = svc.remove_job(&job.id);
        assert!(removed);
        assert_eq!(svc.list_jobs(false).len(), 0);

        // File should exist and be valid JSON
        assert!(path.exists());
        let text = std::fs::read_to_string(&path).unwrap();
        let store: CronStore = serde_json::from_str(&text).unwrap();
        assert_eq!(store.jobs.len(), 0);
    }
}
