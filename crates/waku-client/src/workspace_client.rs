use uuid::Uuid;
use waku_protocol::provider_session::{ProviderSessionFork, ProviderSessionForkRequest};
use waku_protocol::{Command, ResponsePayload, WorkspaceOperation, WorkspaceResult};

use crate::DaemonClient;

/// Typed convenience wrapper around daemon-owned filesystem and Git RPCs.
#[derive(Clone)]
pub struct WorkspaceClient {
    client: DaemonClient,
}

impl WorkspaceClient {
    pub fn new(client: DaemonClient) -> Self {
        Self { client }
    }

    pub fn request(&self, operation: WorkspaceOperation) -> anyhow::Result<WorkspaceResult> {
        // A repository's FIFO may outlast the ordinary RPC timeout. Keep the
        // result attached rather than reporting failure while it still lands.
        let waits_for_landing = matches!(operation, WorkspaceOperation::Land { .. });
        let command = Command::Workspace { operation };
        let response = if waits_for_landing {
            self.client
                .request_with_timeout(Uuid::nil(), Uuid::nil(), command, None)?
        } else {
            self.client.request(Uuid::nil(), Uuid::nil(), command)?
        };
        match response {
            ResponsePayload::Workspace { result } => Ok(result),
            _ => anyhow::bail!("Goddard daemon returned an invalid workspace response"),
        }
    }

    pub fn fork_provider_session(
        &self,
        request: ProviderSessionForkRequest,
    ) -> anyhow::Result<ProviderSessionFork> {
        match self.client.request(
            Uuid::nil(),
            Uuid::nil(),
            Command::ForkProviderSession { request },
        )? {
            ResponsePayload::ProviderSessionForked { result } => Ok(result),
            _ => anyhow::bail!("Goddard daemon returned an invalid provider-fork response"),
        }
    }
}
