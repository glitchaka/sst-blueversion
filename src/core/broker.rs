//! Closed wire contract. No executable, command, script, environment or file operation.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

pub const VERSION: u32 = 1;
pub const MAX_FRAME: usize = 8192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessIdentity {
    pub pid: u32,
    /// Exact Windows FILETIME (100 ns since 1601), NOT sysinfo's Unix seconds.
    pub created: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Operation {
    Inspect,
    Suspend,
    Resume,
    Kill,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    pub nonce: [u8; 16],
    pub client: ProcessIdentity,
    pub session: u32,
    pub operation: Operation,
    pub target: ProcessIdentity,
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.version == VERSION, "unsupported broker protocol");
        ensure!(self.nonce != [0; 16], "invalid request nonce");
        ensure!(self.session > 0, "interactive session required");
        ensure!(
            self.client.pid > 4 && self.client.created > 0,
            "invalid client identity"
        );
        ensure!(
            self.target.pid > 4 && self.target.created > 0,
            "invalid target identity"
        );
        ensure!(
            self.target.pid != self.client.pid,
            "self targeting is forbidden"
        );
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", deny_unknown_fields)]
pub enum Outcome {
    Inspected {
        image: String,
        critical: bool,
        protection: u32,
        session: u32,
    },
    Completed,
    Rejected {
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u32,
    pub nonce: [u8; 16],
    pub operation: Operation,
    pub target: ProcessIdentity,
    pub outcome: Outcome,
}

impl Response {
    pub fn validate(&self, request: &Request) -> Result<()> {
        ensure!(
            self.version == VERSION && self.nonce == request.nonce,
            "broker response version/nonce mismatch"
        );
        ensure!(
            self.target == request.target && self.operation == request.operation,
            "broker response operation/identity mismatch"
        );
        match &self.outcome {
            Outcome::Inspected { image, .. } => {
                ensure!(
                    request.operation == Operation::Inspect,
                    "unexpected inspect response"
                );
                ensure!(
                    !image.is_empty()
                        && image.len() <= 4096
                        && !image.chars().any(char::is_control),
                    "invalid image in response"
                );
            }
            Outcome::Completed => ensure!(
                request.operation != Operation::Inspect,
                "missing inspect result"
            ),
            Outcome::Rejected { reason } => ensure!(
                !reason.is_empty() && reason.len() <= 2048 && !reason.chars().any(char::is_control),
                "invalid rejection"
            ),
        }
        Ok(())
    }
}
