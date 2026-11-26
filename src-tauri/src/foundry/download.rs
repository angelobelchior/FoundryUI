use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use foundry_local_sdk::NativeErrorCode;

use super::error::FoundryError;
use super::jobs::{JobRegistry, JobStatus, ModelJob, now_millis};
use super::json::ModelDetails;
use super::model_cache::{model_directory_size, model_slug, remove_model_directory};
use super::sdk_backend::SdkBackend;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
const CANCEL_INTERVAL: Duration = Duration::from_millis(200);

pub struct DownloadRequest {
    pub cli: SdkBackend,
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
    let total = job.total_bytes;

    job.updated_at = now_millis();
    on_update(job.clone());

    remove_model_directory(&cache_root, &slug);

    let target = if model.id.trim().is_empty() {
        alias.as_str()
    } else {
        model.id.as_str()
    };

    let sdk_model = match cli.model(target).await {
        Ok(model) => model,
        Err(error) => {
            job.status = JobStatus::Failed;
            job.error = Some(error);
            job.updated_at = now_millis();
            jobs.finish(job.clone());
            on_update(job);

            return;
        }
    };

    let received = Arc::new(AtomicU64::new(0));
    let received_cb = received.clone();
    let cancel_flag = cancellation.clone();

    let mut download_task = tokio::spawn(async move {
        sdk_model
            .download_builder()
            .progress(move |percent| {
                received_cb.store(
                    (total as f64 * percent / 100.0).round() as u64,
                    Ordering::SeqCst,
                );
            })
            .cancel(cancel_flag)
            .run()
            .await
    });

    let mut last_sample = Instant::now();
    let mut last_bytes = 0_u64;
    let mut speed = 0.0;

    let joined = loop {
        tokio::select! {
            joined = &mut download_task => break joined,
            _ = tokio::time::sleep(CANCEL_INTERVAL) => {
                let observed_at = Instant::now();

                if observed_at.duration_since(last_sample) < SAMPLE_INTERVAL {
                    continue;
                }

                let current = received.load(Ordering::SeqCst);
                let elapsed = observed_at.duration_since(last_sample).as_secs_f64();
                let instant_speed = current.saturating_sub(last_bytes) as f64 / elapsed.max(0.001);

                speed = if speed <= 0.0 {
                    instant_speed
                } else {
                    speed * 0.6 + instant_speed * 0.4
                };
                last_bytes = last_bytes.max(current);
                job.received_bytes = last_bytes;
                job.bytes_per_second = speed;
                job.eta_seconds = estimate_eta(last_bytes, job.total_bytes, speed);
                job.updated_at = now_millis();
                jobs.update(job.clone());
                on_update(job.clone());

                last_sample = observed_at;
            }
        }
    };

    let disk = sample_size(cache_root.clone(), slug.clone()).await;

    job.received_bytes = job
        .received_bytes
        .max(disk)
        .max(received.load(Ordering::SeqCst));

    match joined {
        Ok(Ok(())) => {
            if job.total_bytes > 0 {
                job.received_bytes = job.total_bytes;
            }

            job.status = JobStatus::Succeeded;
            job.bytes_per_second = 0.0;
            job.eta_seconds = Some(0);
            job.error = None;
        }
        Ok(Err(error)) => {
            if error.native_code() == Some(NativeErrorCode::OperationCancelled) {
                remove_model_directory(&cache_root, &slug);
                job.status = JobStatus::Cancelled;
                job.bytes_per_second = 0.0;
                job.eta_seconds = None;
                job.error = None;
            } else {
                job.status = JobStatus::Failed;
                job.bytes_per_second = 0.0;
                job.eta_seconds = None;
                job.error = Some(super::sdk_backend::map_error(
                    "model download",
                    error,
                ));
            }
        }
        Err(_) => {
            job.status = JobStatus::Failed;
            job.bytes_per_second = 0.0;
            job.eta_seconds = None;
            job.error = Some(FoundryError::CommandFailed {
                command: "foundry model download".to_string(),
                message: "the download task terminated unexpectedly".to_string(),
                hint: None,
            });
        }
    }

    job.updated_at = now_millis();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_remaining_time_from_the_current_speed() {
        assert_eq!(estimate_eta(0, 1024, 512.0), Some(2));
        assert_eq!(estimate_eta(512, 1024, 0.0), None);
        assert_eq!(estimate_eta(1024, 1024, 10.0), Some(0));
        assert_eq!(estimate_eta(0, 0, 10.0), None);
    }
}
