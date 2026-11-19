use async_trait::async_trait;

use super::{ApprovalChoice, ApprovalProvider};
use crate::foundry::db::agent::AgentStore;
use crate::foundry::db::agent::authorization::{AuthorizationRequest, AuthorizationStatus};

#[derive(Default)]
pub struct PersistedApprovals;

#[async_trait]
impl ApprovalProvider for PersistedApprovals {
    fn choose(&mut self, _request: &AuthorizationRequest) -> ApprovalChoice {
        ApprovalChoice::Suspend
    }

    async fn wait(
        &mut self,
        store: &AgentStore<'_>,
        request: &AuthorizationRequest,
    ) -> Result<ApprovalChoice, String> {
        loop {
            let current = store
                .list_requests(request.conversation_id)
                .map_err(|_| "authorization state is unavailable".to_string())?
                .into_iter()
                .find(|current| current.id == request.id)
                .ok_or_else(|| "the authorization no longer exists".to_string())?;
            if current.run_id != request.run_id
                || current.action_id != request.action_id
                || current.context_revision != request.context_revision
                || current.authorization_revision != request.authorization_revision
                || current.mode_revision != request.mode_revision
            {
                return Err("the authorization binding changed".into());
            }
            match current.status {
                AuthorizationStatus::Pending => {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                AuthorizationStatus::Ready
                | AuthorizationStatus::Denied
                | AuthorizationStatus::Invalidated => {
                    return Ok(ApprovalChoice::Refresh);
                }
                AuthorizationStatus::Claimed => {
                    return Err("the authorization is no longer current".into());
                }
            }
        }
    }
}
