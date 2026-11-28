use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ready,
    Running,
    WaitingClarification,
    WaitingApproval,
    WaitingDraftReview,
    NeedsReconciliation,
    Completed,
    Failed,
    Cancelled,
    LimitReached,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    PreparingConversation,
    CheckingDraftEvidence,
    ReviewingDraftEvidence,
    WaitingForModel,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "state", content = "status", rename_all = "snake_case")]
pub enum ToolState {
    Requested,
    Running,
    ApprovalRequired,
    ResultRecorded,
    Superseded,
    Stopped(Status),
    Failed,
}

impl fmt::Display for ToolState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Requested => "Requested",
            Self::Running => "Running",
            Self::ApprovalRequired => "Approval required",
            Self::ResultRecorded => "Result recorded",
            Self::Superseded => "Superseded by correction",
            Self::Stopped(status) => return write!(formatter, "Stopped — {status:?}"),
            Self::Failed => "Failed",
        })
    }
}
