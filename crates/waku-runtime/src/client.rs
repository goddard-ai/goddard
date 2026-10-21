//! The typed IPC client every runtime consumer shares.
//!
//! One `RuntimeClient` wraps the framed stream; the constructor's
//! `ConnectAuth` picks the role and the server enforces it thereafter.
//! Every method is a single request/response — a torn connection surfaces
//! as `ClientError::Io`, and the caller's answer to a lost response is to
//! query the recorded truth (`command`, `ingress_query`), never to invent
//! a new identity.

use std::io;
use std::os::unix::net::UnixStream;
use std::path::Path;

use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

use crate::contract::*;
use crate::ipc::{self, Endpoint};
use crate::wire::*;

#[derive(Debug, Error)]
pub enum ClientError {
    /// The host refused — authorization, fencing, or precondition truth.
    #[error("{0}")]
    Host(#[from] HostError),
    #[error("transport: {0}")]
    Io(#[from] io::Error),
    /// The peer answered outside the protocol.
    #[error("protocol violation: {0}")]
    Protocol(String),
}

pub struct RuntimeClient {
    stream: UnixStream,
}

fn payload<T>(
    response: WireResponse,
    extract: impl FnOnce(WireResponse) -> Option<T>,
) -> Result<T, ClientError> {
    match response {
        WireResponse::Error { error } => Err(ClientError::Host(error)),
        other => extract(other).ok_or_else(|| ClientError::Protocol("unexpected response".into())),
    }
}

impl RuntimeClient {
    /// Connect and perform the `Connect` handshake. The endpoint record is
    /// read for the namespace; the server re-verifies it against the live
    /// host, so a stale or foreign record cannot open a session.
    pub fn connect(root: &Path, auth: ConnectAuth) -> Result<(Self, WireResponse), ClientError> {
        let endpoint = Endpoint::load(root).map_err(ClientError::Io)?;
        // The endpoint records where the server actually bound — trust
        // the record over re-deriving the path.
        let socket = if matches!(&auth, ConnectAuth::Emergency { .. }) {
            endpoint
                .emergency_socket
                .as_deref()
                .unwrap_or(&endpoint.socket)
        } else {
            &endpoint.socket
        };
        let mut stream = UnixStream::connect(socket).map_err(ClientError::Io)?;
        if !ipc::same_user(&stream)? {
            return Err(HostError::Unauthorized.into());
        }
        let agent = matches!(&auth, ConnectAuth::Agent { .. });
        stream.set_read_timeout(Some(std::time::Duration::from_secs(if agent {
            10
        } else {
            35
        })))?;
        stream.set_write_timeout(Some(std::time::Duration::from_secs(5)))?;
        ipc::send(
            &mut stream,
            &Connect {
                namespace: endpoint.namespace.clone(),
                auth,
            },
        )?;
        let response = ipc::recv::<WireResponse>(&mut stream)?
            .ok_or_else(|| ClientError::Protocol("connection closed during handshake".into()))?;
        if let WireResponse::Error { error } = response {
            return Err(ClientError::Host(error));
        }
        stream.set_read_timeout(Some(std::time::Duration::from_secs(35)))?;
        let identity = match &response {
            WireResponse::Hello { hello } => {
                Some((hello.runtime_instance, hello.namespace.as_str()))
            }
            WireResponse::Status { status } => {
                Some((status.runtime_instance, endpoint.namespace.as_str()))
            }
            _ => None,
        };
        if identity.is_some_and(|(instance, namespace)| {
            instance != endpoint.instance || namespace != endpoint.namespace
        }) {
            return Err(HostError::Unauthorized.into());
        }
        Ok((Self { stream }, response))
    }

    /// Attach as the controller (the product daemon).
    pub fn controller(
        root: &Path,
        credential: &str,
        client: &ClientHello,
    ) -> Result<(Self, Hello), ClientError> {
        let (this, response) = Self::connect(
            root,
            ConnectAuth::Controller {
                credential: credential.to_string(),
                client: client.clone(),
            },
        )?;
        let hello = payload(response, |r| match r {
            WireResponse::Hello { hello } => Some(*hello),
            _ => None,
        })?;
        Ok((this, hello))
    }

    /// Attach as the daemon's service endpoint for forwarded agent
    /// traffic, bound to the epoch it serves.
    pub fn service(
        root: &Path,
        credential: &str,
        epoch: u64,
        client: &ClientHello,
    ) -> Result<(Self, Hello), ClientError> {
        let (this, response) = Self::connect(
            root,
            ConnectAuth::Service {
                credential: credential.to_string(),
                epoch,
                client: client.clone(),
            },
        )?;
        let hello = payload(response, |r| match r {
            WireResponse::Hello { hello } => Some(*hello),
            _ => None,
        })?;
        Ok((this, hello))
    }

    /// Attach with narrow emergency authority.
    pub fn emergency(root: &Path, credential: &str) -> Result<(Self, HostStatus), ClientError> {
        let (this, response) = Self::connect(
            root,
            ConnectAuth::Emergency {
                credential: credential.to_string(),
            },
        )?;
        let status = payload(response, |r| match r {
            WireResponse::Status { status } => Some(*status),
            _ => None,
        })?;
        Ok((this, status))
    }

    /// Attach under a session-scoped ingress grant.
    pub fn agent(root: &Path, grant: &SessionGrant) -> Result<Self, ClientError> {
        let (this, response) = Self::connect(
            root,
            ConnectAuth::Agent {
                grant: grant.clone(),
            },
        )?;
        payload(response, |r| matches!(r, WireResponse::Ok).then_some(()))?;
        Ok(this)
    }

    fn call(&mut self, request: &WireRequest) -> Result<WireResponse, ClientError> {
        ipc::send(&mut self.stream, request)?;
        ipc::recv::<WireResponse>(&mut self.stream)?
            .ok_or_else(|| ClientError::Protocol("connection closed".into()))
    }

    /// Bound transport I/O by the caller's remaining request deadline.
    /// Long-poll waits must fit inside this bound; zero is refused.
    pub fn set_request_timeout(&mut self, timeout: std::time::Duration) -> Result<(), ClientError> {
        if timeout.is_zero() {
            return Err(ClientError::Protocol(
                "request timeout must be positive".into(),
            ));
        }
        self.stream.set_read_timeout(Some(timeout))?;
        self.stream
            .set_write_timeout(Some(timeout.min(std::time::Duration::from_secs(5))))?;
        Ok(())
    }

    pub fn hello(&mut self, client: &ClientHello) -> Result<Hello, ClientError> {
        let response = self.call(&WireRequest::Hello {
            client: client.clone(),
        })?;
        payload(response, |r| match r {
            WireResponse::Hello { hello } => Some(*hello),
            _ => None,
        })
    }

    pub fn acquire_control(&mut self, client: &ClientHello) -> Result<ControlGrant, ClientError> {
        let response = self.call(&WireRequest::AcquireControl {
            client: client.clone(),
        })?;
        payload(response, |r| match r {
            WireResponse::Control { grant } => Some(grant),
            _ => None,
        })
    }

    pub fn reconcile(&mut self, epoch: u64) -> Result<Inventory, ClientError> {
        let response = self.call(&WireRequest::Reconcile { epoch })?;
        payload(response, |r| match r {
            WireResponse::Inventory { inventory } => Some(*inventory),
            _ => None,
        })
    }

    pub fn execute(&mut self, envelope: ActionEnvelope) -> Result<CommandReceipt, ClientError> {
        let response = self.call(&WireRequest::Execute {
            envelope: Box::new(envelope),
        })?;
        payload(response, |r| match r {
            WireResponse::Receipt { receipt } => Some(receipt),
            _ => None,
        })
    }

    /// The recorded truth for a command — the answer to a lost response.
    pub fn command(&mut self, command_id: Uuid) -> Result<Option<CommandReceipt>, ClientError> {
        let response = self.call(&WireRequest::Command { command_id })?;
        payload(response, |r| match r {
            WireResponse::Command { receipt } => Some(receipt),
            _ => None,
        })
    }

    /// Replay from a durable cursor, optionally long-polling for the next
    /// events. `wait_ms: 0` always returns immediately.
    pub fn events(
        &mut self,
        epoch: u64,
        cursor: EventCursor,
        wait_ms: u64,
    ) -> Result<EventPage, ClientError> {
        let response = self.call(&WireRequest::Events {
            epoch,
            cursor,
            wait_ms,
        })?;
        payload(response, |r| match r {
            WireResponse::Page { page } => Some(*page),
            _ => None,
        })
    }

    pub fn acknowledge(&mut self, epoch: u64, cursor: EventCursor) -> Result<(), ClientError> {
        let response = self.call(&WireRequest::Acknowledge { epoch, cursor })?;
        payload(response, |r| matches!(r, WireResponse::Ok).then_some(()))
    }

    pub fn status(&mut self) -> Result<HostStatus, ClientError> {
        let response = self.call(&WireRequest::Status)?;
        payload(response, |r| match r {
            WireResponse::Status { status } => Some(*status),
            _ => None,
        })
    }

    pub fn drain(&mut self, epoch: u64, policy: DrainPolicy) -> Result<DrainReport, ClientError> {
        let response = self.call(&WireRequest::Drain { epoch, policy })?;
        payload(response, |r| match r {
            WireResponse::Drain { report } => Some(report),
            _ => None,
        })
    }

    pub fn emergency_stop(&mut self, session_id: Uuid) -> Result<SessionStatus, ClientError> {
        let response = self.call(&WireRequest::EmergencyStop { session_id })?;
        payload(response, |r| match r {
            WireResponse::SessionStatus { status } => Some(status),
            _ => None,
        })
    }

    pub fn issue_grant(
        &mut self,
        epoch: u64,
        session: SessionRef,
        ttl_ms: u64,
    ) -> Result<SessionGrant, ClientError> {
        let response = self.call(&WireRequest::IssueGrant {
            epoch,
            session,
            ttl_ms,
        })?;
        payload(response, |r| match r {
            WireResponse::Grant { grant } => Some(*grant),
            _ => None,
        })
    }

    pub fn register_claim(&mut self, epoch: u64, claim: ResourceClaim) -> Result<(), ClientError> {
        let response = self.call(&WireRequest::RegisterClaim { epoch, claim })?;
        payload(response, |r| matches!(r, WireResponse::Ok).then_some(()))
    }

    pub fn release_claim(&mut self, epoch: u64, claim_id: Uuid) -> Result<(), ClientError> {
        let response = self.call(&WireRequest::ReleaseClaim { epoch, claim_id })?;
        payload(response, |r| matches!(r, WireResponse::Ok).then_some(()))
    }

    /// The service endpoint's drain — the forwarded agent traffic the
    /// current epoch must answer.
    pub fn service_drain(
        &mut self,
        epoch: u64,
        wait_ms: u64,
    ) -> Result<Vec<ForwardedRequest>, ClientError> {
        let response = self.call(&WireRequest::ServiceDrain { epoch, wait_ms })?;
        payload(response, |r| match r {
            WireResponse::Forwarded { requests } => Some(requests),
            _ => None,
        })
    }

    pub fn service_result(
        &mut self,
        epoch: u64,
        request_id: Uuid,
        outcome: ForwardedOutcome,
    ) -> Result<IngressRecord, ClientError> {
        let response = self.call(&WireRequest::ServiceResult {
            epoch,
            request_id,
            outcome,
        })?;
        payload(response, |r| match r {
            WireResponse::Ingress { record } => Some(*record),
            _ => None,
        })
    }

    /// Agent: submit one request under the connection's grant.
    pub fn ingress(&mut self, request: IngressRequest) -> Result<IngressRecord, ClientError> {
        let response = self.call(&WireRequest::Ingress { request })?;
        payload(response, |r| match r {
            WireResponse::Ingress { record } => Some(*record),
            _ => None,
        })
    }

    /// Agent: read the recorded truth for one of its own requests — the
    /// answer to a lost response.
    pub fn ingress_query(&mut self, request_id: Uuid) -> Result<IngressRecord, ClientError> {
        let response = self.call(&WireRequest::IngressQuery { request_id })?;
        payload(response, |r| match r {
            WireResponse::Ingress { record } => Some(*record),
            _ => None,
        })
    }

    /// Agent: wait for a request to leave the live dispositions.
    /// `wait_ms: 0` always returns immediately.
    pub fn ingress_wait(
        &mut self,
        request_id: Uuid,
        wait_ms: u64,
    ) -> Result<IngressRecord, ClientError> {
        let response = self.call(&WireRequest::IngressWait {
            request_id,
            wait_ms,
        })?;
        payload(response, |r| match r {
            WireResponse::Ingress { record } => Some(*record),
            _ => None,
        })
    }
}

/// Convenience: a submit's result the daemon can record as its own
/// forwarded outcome — the payload it returns to the agent unchanged.
pub fn forwarded_ok(result: Value) -> ForwardedOutcome {
    ForwardedOutcome::Completed { result }
}
