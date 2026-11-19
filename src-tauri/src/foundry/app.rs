use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::attachments::AttachmentStore;
use super::chat::GenerationRegistry;
use super::cli::CliBackend;
use super::db::Database;
use super::error::{FoundryError, FoundryResult};
use super::jobs::JobRegistry;
use super::project_selection::ProjectSelectionRegistry;
use super::server::{ServerManager, ServerPreferences};
use super::speech::Recorder;
use super::vectorize::VectorizeRegistry;

pub struct AppState {
    cli: RwLock<Option<CliBackend>>,
    database: RwLock<Option<Arc<Database>>>,
    attachments: RwLock<Option<Arc<AttachmentStore>>>,
    pub server: ServerManager,
    pub jobs: Arc<JobRegistry>,
    pub generations: Arc<GenerationRegistry>,
    pub recorder: Recorder,
    pub vectorizations: Arc<VectorizeRegistry>,
    pub(crate) project_selections: ProjectSelectionRegistry,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            cli: RwLock::new(None),
            database: RwLock::new(None),
            attachments: RwLock::new(None),
            server: ServerManager::new(ServerPreferences::default()),
            jobs: Arc::new(JobRegistry::default()),
            generations: Arc::new(GenerationRegistry::default()),
            recorder: Recorder::new(),
            vectorizations: Arc::new(VectorizeRegistry::default()),
            project_selections: ProjectSelectionRegistry::default(),
        }
    }

    pub fn install_database(&self, database: Arc<Database>) {
        *write(&self.database) = Some(database);
    }

    pub fn install_attachments(&self, store: Arc<AttachmentStore>) {
        *write(&self.attachments) = Some(store);
    }

    pub fn attachments(&self) -> FoundryResult<Arc<AttachmentStore>> {
        read(&self.attachments)
            .clone()
            .ok_or_else(|| FoundryError::Storage {
                detail: "the attachment store is not ready".to_string(),
            })
    }

    pub fn database(&self) -> FoundryResult<Arc<Database>> {
        read(&self.database)
            .clone()
            .ok_or_else(|| FoundryError::Storage {
                detail: "the local database is not open".to_string(),
            })
    }

    pub(crate) fn with_database<T>(
        &self,
        operation: impl FnOnce(&Arc<Database>) -> T,
    ) -> FoundryResult<T> {
        let database = read(&self.database);
        database
            .as_ref()
            .map(operation)
            .ok_or_else(|| FoundryError::Storage {
                detail: "the local database is not open".to_string(),
            })
    }

    pub fn backend(&self) -> FoundryResult<CliBackend> {
        if let Some(cli) = read(&self.cli).clone() {
            return Ok(cli);
        }

        let cli = CliBackend::detect()?;
        *write(&self.cli) = Some(cli.clone());

        Ok(cli)
    }

    pub fn refresh_backend(&self) -> FoundryResult<CliBackend> {
        self.server.invalidate_prepared_context();
        let cli = CliBackend::detect()?;
        *write(&self.cli) = Some(cli.clone());
        self.server.invalidate_prepared_context();

        Ok(cli)
    }

    pub fn cached_backend(&self) -> Option<CliBackend> {
        read(&self.cli).clone()
    }

    pub fn shutdown(&self) {
        if !self.server.should_stop_on_exit() {
            return;
        }

        if let Some(cli) = self.cached_backend() {
            cli.stop_blocking();
            self.server.mark_stopped();
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}
