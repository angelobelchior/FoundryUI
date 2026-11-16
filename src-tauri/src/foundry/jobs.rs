use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use super::error::FoundryError;

pub const MODEL_JOB_EVENT: &str = "foundry-model-job";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum JobStatus {
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelJob {
    pub alias: String,
    pub status: JobStatus,
    pub received_bytes: u64,
    pub total_bytes: u64,
    pub bytes_per_second: f64,
    pub eta_seconds: Option<u64>,
    pub started_at: i64,
    pub updated_at: i64,
    pub error: Option<FoundryError>,
}

impl ModelJob {
    pub fn starting_download(alias: &str, total_bytes: u64) -> Self {
        let now = now_millis();

        Self {
            alias: alias.to_string(),
            status: JobStatus::Running,
            received_bytes: 0,
            total_bytes,
            bytes_per_second: 0.0,
            eta_seconds: None,
            started_at: now,
            updated_at: now,
            error: None,
        }
    }

    pub fn already_cached(alias: &str, total_bytes: u64) -> Self {
        let now = now_millis();

        Self {
            alias: alias.to_string(),
            status: JobStatus::Succeeded,
            received_bytes: total_bytes,
            total_bytes,
            bytes_per_second: 0.0,
            eta_seconds: Some(0),
            started_at: now,
            updated_at: now,
            error: None,
        }
    }
}

#[derive(Debug, Default)]
pub struct JobRegistry {
    jobs: Mutex<HashMap<String, ModelJob>>,
    cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
    loaded_since: Mutex<HashMap<String, i64>>,
}

impl JobRegistry {
    pub fn begin_download(
        &self,
        alias: &str,
        total_bytes: u64,
    ) -> Option<(ModelJob, Arc<AtomicBool>)> {
        let mut jobs = lock(&self.jobs);

        if jobs
            .get(alias)
            .is_some_and(|job| job.status == JobStatus::Running)
        {
            return None;
        }

        let job = ModelJob::starting_download(alias, total_bytes);
        jobs.insert(alias.to_string(), job.clone());

        let cancellation = Arc::new(AtomicBool::new(false));
        lock(&self.cancellations).insert(alias.to_string(), cancellation.clone());

        Some((job, cancellation))
    }

    pub fn update(&self, job: ModelJob) {
        lock(&self.jobs).insert(job.alias.clone(), job);
    }

    pub fn finish(&self, job: ModelJob) {
        let alias = job.alias.clone();

        lock(&self.jobs).insert(alias.clone(), job);
        lock(&self.cancellations).remove(&alias);
    }

    pub fn job(&self, alias: &str) -> Option<ModelJob> {
        lock(&self.jobs).get(alias).cloned()
    }

    pub fn jobs(&self) -> Vec<ModelJob> {
        let mut jobs: Vec<ModelJob> = lock(&self.jobs).values().cloned().collect();

        jobs.sort_by_key(|job| job.started_at);

        jobs
    }

    pub fn cancel(&self, alias: &str) -> bool {
        match lock(&self.cancellations).get(alias) {
            Some(cancellation) => {
                cancellation.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn sync_loaded(&self, aliases: &[String]) {
        let mut loaded = lock(&self.loaded_since);
        let now = now_millis();

        loaded.retain(|alias, _| aliases.iter().any(|value| value == alias));

        for alias in aliases {
            loaded.entry(alias.clone()).or_insert(now);
        }
    }

    pub fn loaded_at(&self, alias: &str) -> Option<i64> {
        lock(&self.loaded_since).get(alias).copied()
    }

    pub fn mark_loaded(&self, alias: &str) {
        lock(&self.loaded_since)
            .entry(alias.to_string())
            .or_insert_with(now_millis);
    }

    pub fn clear_loaded(&self, alias: &str) {
        lock(&self.loaded_since).remove(alias);
    }
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_a_second_download_for_the_same_alias() {
        let registry = JobRegistry::default();
        let (job, _) = registry
            .begin_download("whisper-tiny", 10)
            .expect("first starts");

        assert_eq!(job.status, JobStatus::Running);
        assert!(registry.begin_download("whisper-tiny", 10).is_none());
    }

    #[test]
    fn cancellation_marks_the_running_job() {
        let registry = JobRegistry::default();
        let (_, cancellation) = registry
            .begin_download("whisper-tiny", 10)
            .expect("first starts");

        assert!(registry.cancel("whisper-tiny"));
        assert!(cancellation.load(Ordering::SeqCst));
        assert!(!registry.cancel("unknown"));
    }

    #[test]
    fn finishing_a_job_releases_the_cancellation_token() {
        let registry = JobRegistry::default();
        let (mut job, _) = registry
            .begin_download("whisper-tiny", 10)
            .expect("first starts");

        job.status = JobStatus::Succeeded;
        registry.finish(job);

        assert!(!registry.cancel("whisper-tiny"));
        assert_eq!(
            registry.job("whisper-tiny").expect("job is kept").status,
            JobStatus::Succeeded
        );
    }

    #[test]
    fn keeps_loaded_timestamps_while_models_stay_resident() {
        let registry = JobRegistry::default();
        let first = vec!["whisper-tiny".to_string()];

        registry.sync_loaded(&first);
        let since = registry
            .loaded_at("whisper-tiny")
            .expect("timestamp is set");

        registry.sync_loaded(&first);
        assert_eq!(registry.loaded_at("whisper-tiny"), Some(since));

        registry.sync_loaded(&[]);
        assert_eq!(registry.loaded_at("whisper-tiny"), None);
    }

    #[test]
    fn an_already_cached_model_reports_a_completed_job() {
        let job = ModelJob::already_cached("whisper-tiny", 42);

        assert_eq!(job.status, JobStatus::Succeeded);
        assert_eq!(job.received_bytes, 42);
        assert_eq!(job.eta_seconds, Some(0));
    }
}
