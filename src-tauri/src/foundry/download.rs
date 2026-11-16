use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::cli::CliBackend;
use super::error::FoundryError;
use super::jobs::{JobRegistry, JobStatus, ModelJob, now_millis};
use super::json::ModelDetails;
use super::model_cache::{model_directory_size, model_slug, remove_model_directory};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
const CANCEL_INTERVAL: Duration = Duration::from_millis(200);

pub struct DownloadRequest {
    pub cli: CliBackend,
    pub jobs: Arc<JobRegistry>,
    pub alias: String,
    pub model: ModelDetails,
    pub cache_root: PathBuf,
    pub job: ModelJob,
    pub cancellation: Arc<AtomicBool>,
}

pub async fn run_download<F>(request: DownloadRequest, on_update: F)
where
    F: Fn(ModelJob) + Send + Sync + 'static,
{
    let DownloadRequest {
        cli,
        jobs,
        alias,
        model,
        cache_root,
        mut job,
        cancellation,
    } = request;
    let slug = model_slug(&model.id);

    job.updated_at = now_millis();
    on_update(job.clone());

    remove_model_directory(&cache_root, &slug);

    let target = if model.id.trim().is_empty() {
        alias.as_str()
    } else {
        model.id.as_str()
    };

    let mut process = match cli.spawn_text(&["model", "download", target, "--force"]) {
        Ok(process) => process,
        Err(error) => {
            job.status = JobStatus::Failed;
            job.error = Some(error);
            job.updated_at = now_millis();
            jobs.finish(job.clone());
            on_update(job);

            return;
        }
    };

    let mut last_sample = Instant::now();
    let mut last_bytes = job.received_bytes;
    let mut speed = 0.0;
    let mut failure: Option<FoundryError> = None;

    loop {
        match process.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    failure = Some(process.exit_error(status));
                }

                break;
            }
            Ok(None) => {}
            Err(error) => {
                process.cancel().await;
                failure = Some(FoundryError::CommandFailed {
                    command: format!("foundry model download {target}"),
                    message: error.to_string(),
                    hint: None,
                });

                break;
            }
        }

        if cancellation.load(Ordering::SeqCst) {
            process.cancel().await;
            remove_model_directory(&cache_root, &slug);
            job.status = JobStatus::Cancelled;
            job.bytes_per_second = 0.0;
            job.eta_seconds = None;
            job.updated_at = now_millis();
            jobs.finish(job.clone());
            on_update(job);

            return;
        }

        if last_sample.elapsed() >= SAMPLE_INTERVAL {
            let disk_received = sample_size(cache_root.clone(), slug.clone()).await;
            let cli_received =
                cli_progress_bytes(&process.captured_stdout(), job.total_bytes).unwrap_or_default();
            let received = disk_received.max(cli_received);
            let observed_at = Instant::now();
            let elapsed = observed_at.duration_since(last_sample).as_secs_f64();
            let instant_speed = received.saturating_sub(last_bytes) as f64 / elapsed.max(0.001);

            speed = if speed <= 0.0 {
                instant_speed
            } else {
                speed * 0.6 + instant_speed * 0.4
            };
            last_bytes = last_bytes.max(received);
            job.received_bytes = last_bytes;
            job.bytes_per_second = speed;
            job.eta_seconds = estimate_eta(last_bytes, job.total_bytes, speed);
            job.updated_at = now_millis();
            jobs.update(job.clone());
            on_update(job.clone());

            last_sample = observed_at;
        }

        tokio::time::sleep(CANCEL_INTERVAL).await;
    }

    let received = sample_size(cache_root.clone(), slug).await;

    job.received_bytes = job.received_bytes.max(received);
    job.updated_at = now_millis();

    if let Some(error) = failure {
        job.status = JobStatus::Failed;
        job.bytes_per_second = 0.0;
        job.eta_seconds = None;
        job.error = Some(error);
    } else if received == 0 {
        job.status = JobStatus::Failed;
        job.bytes_per_second = 0.0;
        job.eta_seconds = None;
        job.error = Some(FoundryError::ModelNotCached {
            alias: alias.clone(),
        });
    } else {
        if job.total_bytes > 0 {
            job.received_bytes = job.total_bytes;
        }

        job.status = JobStatus::Succeeded;
        job.bytes_per_second = 0.0;
        job.eta_seconds = Some(0);
        job.error = None;
    }

    jobs.finish(job.clone());
    on_update(job);
}

async fn sample_size(root: PathBuf, slug: String) -> u64 {
    tokio::task::spawn_blocking(move || model_directory_size(&root, &slug))
        .await
        .unwrap_or_default()
}

fn estimate_eta(received: u64, total: u64, speed: f64) -> Option<u64> {
    if total == 0 || speed <= 1.0 {
        return None;
    }

    if received >= total {
        return Some(0);
    }

    Some(((total - received) as f64 / speed).ceil() as u64)
}

fn cli_progress_bytes(output: &str, total_bytes: u64) -> Option<u64> {
    if total_bytes == 0 {
        return None;
    }

    output.lines().rev().find_map(|line| {
        let (_, value) = line.rsplit_once(':')?;
        let percent = value.trim().strip_suffix('%')?.trim().parse::<u64>().ok()?;

        Some(total_bytes.saturating_mul(percent.min(100)) / 100)
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Mutex, MutexGuard};

    use super::*;
    use crate::foundry::backend::FoundryBackend;
    use crate::foundry::cli::CliBackend;

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn fake_cli(script: &str) -> (tempfile::TempDir, CliBackend) {
        let directory = tempfile::tempdir().expect("script directory");
        let binary = directory.path().join("foundry");
        let body = format!("#!/bin/sh\n{script}\n");

        fs::write(&binary, body).expect("script is written");
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).expect("script runs");

        let backend = CliBackend::detect_from(&[binary]).expect("fake cli is detected");

        (directory, backend)
    }

    fn model(id: &str, megabytes: f64) -> ModelDetails {
        ModelDetails {
            alias: "fake".to_string(),
            id: id.to_string(),
            file_size_mb: Some(megabytes),
            ..ModelDetails::default()
        }
    }

    #[tokio::test]
    async fn reports_progress_and_finishes_after_the_cli_exits() {
        let cache = tempfile::tempdir().expect("cache root");
        let target = cache
            .path()
            .join("Microsoft")
            .join("fake-model-1")
            .join("v1");
        let script = format!(
            "if [ \"$1\" = \"--version\" ]; then echo 0.13.0; exit 0; fi\nmkdir -p \"{}\"\nfor i in 1 2 3 4 5 6; do dd if=/dev/zero of=\"{}/part$i\" bs=1024 count=64 2>/dev/null; sleep 0.2; done",
            target.display(),
            target.display()
        );
        let (_script, cli) = fake_cli(&script);
        let jobs = Arc::new(JobRegistry::default());
        let (job, cancellation) = jobs
            .begin_download("fake", 384 * 1024)
            .expect("download starts");
        let updates = Arc::new(Mutex::new(Vec::new()));
        let recorder = updates.clone();

        run_download(
            DownloadRequest {
                cli,
                jobs: jobs.clone(),
                alias: "fake".to_string(),
                model: model("fake-model:1", 0.375),
                cache_root: cache.path().to_path_buf(),
                job,
                cancellation,
            },
            move |update| lock(&recorder).push(update),
        )
        .await;

        let final_job = jobs.job("fake").expect("job is kept");
        let seen = lock(&updates).len();

        assert_eq!(final_job.status, JobStatus::Succeeded);
        assert_eq!(final_job.received_bytes, final_job.total_bytes);
        assert!(seen >= 2, "progress is reported while the download runs");
        assert!(
            lock(&updates)
                .iter()
                .any(|update| update.received_bytes > 0 && update.status == JobStatus::Running)
        );
    }

    #[tokio::test]
    async fn reports_cli_progress_before_the_cache_file_is_materialized() {
        let cache = tempfile::tempdir().expect("cache root");
        let target = cache
            .path()
            .join("Microsoft")
            .join("fake-model-1")
            .join("v1");
        let script = format!(
            "if [ \"$1\" = \"--version\" ]; then echo 0.13.0; exit 0; fi\necho 'fake-model:1: 25%'\nsleep 0.6\necho 'fake-model:1: 50%'\nsleep 0.6\nmkdir -p \"{}\"\ndd if=/dev/zero of=\"{}/weights.onnx\" bs=1024 count=64 2>/dev/null",
            target.display(),
            target.display()
        );
        let (_script, cli) = fake_cli(&script);
        let jobs = Arc::new(JobRegistry::default());
        let (job, cancellation) = jobs
            .begin_download("fake", 64 * 1024)
            .expect("download starts");
        let updates = Arc::new(Mutex::new(Vec::new()));
        let recorder = updates.clone();

        run_download(
            DownloadRequest {
                cli,
                jobs,
                alias: "fake".to_string(),
                model: model("fake-model:1", 0.0625),
                cache_root: cache.path().to_path_buf(),
                job,
                cancellation,
            },
            move |update| lock(&recorder).push(update),
        )
        .await;

        assert!(lock(&updates).iter().any(|update| {
            update.status == JobStatus::Running && update.received_bytes == 32 * 1024
        }));
    }

    #[tokio::test]
    async fn fails_when_the_cli_exits_successfully_without_caching_the_model() {
        let cache = tempfile::tempdir().expect("cache root");
        let (_script, cli) =
            fake_cli("if [ \"$1\" = \"--version\" ]; then echo 0.13.0; exit 0; fi\nexit 0");
        let jobs = Arc::new(JobRegistry::default());
        let (job, cancellation) = jobs
            .begin_download("fake", 64 * 1024)
            .expect("download starts");

        run_download(
            DownloadRequest {
                cli,
                jobs: jobs.clone(),
                alias: "fake".to_string(),
                model: model("fake-model:1", 0.0625),
                cache_root: cache.path().to_path_buf(),
                job,
                cancellation,
            },
            |_| {},
        )
        .await;

        let final_job = jobs.job("fake").expect("job is kept");

        assert_eq!(final_job.status, JobStatus::Failed);
        assert!(final_job.error.is_some());
    }

    #[tokio::test]
    async fn cancellation_kills_the_child_and_cleans_the_partial_model() {
        let cache = tempfile::tempdir().expect("cache root");
        let target = cache
            .path()
            .join("Microsoft")
            .join("fake-model-1")
            .join("v1");
        let script = format!(
            "if [ \"$1\" = \"--version\" ]; then echo 0.13.0; exit 0; fi\nmkdir -p \"{}\"\ndd if=/dev/zero of=\"{}/weights.onnx\" bs=1024 count=64 2>/dev/null\nsleep 10",
            target.display(),
            target.display()
        );
        let (_script, cli) = fake_cli(&script);
        let jobs = Arc::new(JobRegistry::default());
        let (job, cancellation) = jobs
            .begin_download("fake", 65_536)
            .expect("download starts");
        let task = tokio::spawn(run_download(
            DownloadRequest {
                cli,
                jobs: jobs.clone(),
                alias: "fake".to_string(),
                model: model("fake-model:1", 0.0625),
                cache_root: cache.path().to_path_buf(),
                job,
                cancellation,
            },
            |_| {},
        ));

        tokio::time::sleep(Duration::from_millis(700)).await;

        assert!(jobs.cancel("fake"));
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("download stops promptly")
            .expect("download task finishes");

        let final_job = jobs.job("fake").expect("job is kept");

        assert_eq!(final_job.status, JobStatus::Cancelled);
        assert!(!target.exists(), "partial download is removed");
    }

    #[test]
    fn estimates_remaining_time_from_the_current_speed() {
        assert_eq!(estimate_eta(0, 1024, 512.0), Some(2));
        assert_eq!(estimate_eta(512, 1024, 0.0), None);
        assert_eq!(estimate_eta(1024, 1024, 10.0), Some(0));
        assert_eq!(estimate_eta(0, 0, 10.0), None);
    }

    #[tokio::test]
    #[ignore = "downloads a real model with the installed foundry CLI"]
    async fn real_cli_download_reports_disk_progress_and_finishes() {
        let cli = match CliBackend::detect() {
            Ok(cli) => cli,
            Err(_) => return,
        };
        let alias = "whisper-tiny";
        let Ok(model) = cli.model_info(alias).await else {
            return;
        };

        if model.cached {
            return;
        }

        let location = cli.cache_location().await.expect("cache location");
        let total_bytes = super::super::json::megabytes_to_bytes(model.file_size_mb);
        let jobs = Arc::new(JobRegistry::default());
        let (job, cancellation) = jobs
            .begin_download(alias, total_bytes)
            .expect("download starts");
        let updates = Arc::new(Mutex::new(Vec::new()));
        let recorder = updates.clone();

        run_download(
            DownloadRequest {
                cli: cli.clone(),
                jobs: jobs.clone(),
                alias: alias.to_string(),
                model,
                cache_root: PathBuf::from(&location.path),
                job,
                cancellation,
            },
            move |update| lock(&recorder).push(update),
        )
        .await;

        let final_job = jobs.job(alias).expect("job is kept");

        assert_eq!(final_job.status, JobStatus::Succeeded);
        assert_eq!(final_job.received_bytes, final_job.total_bytes);
        assert!(
            lock(&updates)
                .iter()
                .any(|update| update.received_bytes > 0 && update.status == JobStatus::Running),
            "progress is sampled from the cache directory"
        );
        assert!(
            cli.cache_list()
                .await
                .expect("cache list")
                .iter()
                .any(|entry| entry.alias == alias && entry.cached)
        );
    }

    #[tokio::test]
    #[ignore = "downloads a real model with the installed foundry CLI"]
    async fn real_cli_download_cancel_leaves_no_partial_model() {
        let cli = match CliBackend::detect() {
            Ok(cli) => cli,
            Err(_) => return,
        };
        let alias = "whisper-small";
        let Ok(model) = cli.model_info(alias).await else {
            return;
        };

        if model.cached {
            return;
        }

        let location = cli.cache_location().await.expect("cache location");
        let total_bytes = super::super::json::megabytes_to_bytes(model.file_size_mb);
        let jobs = Arc::new(JobRegistry::default());
        let (job, cancellation) = jobs
            .begin_download(alias, total_bytes)
            .expect("download starts");
        let task = tokio::spawn(run_download(
            DownloadRequest {
                cli: cli.clone(),
                jobs: jobs.clone(),
                alias: alias.to_string(),
                model,
                cache_root: PathBuf::from(&location.path),
                job,
                cancellation,
            },
            |_| {},
        ));

        tokio::time::sleep(Duration::from_millis(800)).await;

        if jobs
            .job(alias)
            .is_none_or(|job| job.status != JobStatus::Running)
        {
            return;
        }

        assert!(jobs.cancel(alias));
        tokio::time::timeout(Duration::from_secs(10), task)
            .await
            .expect("download stops promptly")
            .expect("download task finishes");

        let final_job = jobs.job(alias).expect("job is kept");

        assert_eq!(final_job.status, JobStatus::Cancelled);
        assert!(
            !cli.cache_list()
                .await
                .expect("cache list")
                .iter()
                .any(|entry| entry.alias == alias)
        );
    }
}
