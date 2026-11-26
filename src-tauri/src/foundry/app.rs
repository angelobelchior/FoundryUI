use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::attachments::AttachmentStore;
use super::chat::GenerationRegistry;
use super::db::Database;
use super::error::{FoundryError, FoundryResult};
use super::jobs::JobRegistry;
use super::project_selection::ProjectSelectionRegistry;
use super::sdk_backend::SdkBackend;
use super::server::{ServerManager, ServerPreferences};
use super::speech::Recorder;
use super::vectorize::VectorizeRegistry;

pub struct AppState {
    backend: RwLock<Option<SdkBackend>>,
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
            backend: RwLock::new(None),
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

    pub fn backend(&self) -> FoundryResult<SdkBackend> {
        if let Some(backend) = read(&self.backend).clone() {
            return Ok(backend);
        }

        let backend = SdkBackend::detect()?;
        *write(&self.backend) = Some(backend.clone());

        Ok(backend)
    }

    pub fn refresh_backend(&self) -> FoundryResult<SdkBackend> {
        let backend = SdkBackend::detect()?;
        *write(&self.backend) = Some(backend.clone());

        Ok(backend)
    }

    pub fn cached_backend(&self) -> Option<SdkBackend> {
        read(&self.backend).clone()
    }

    pub fn shutdown(&self) {
        self.server.mark_stopped();
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
