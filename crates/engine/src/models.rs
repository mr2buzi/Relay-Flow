use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub name: String,
    pub description: Option<String>,
    #[serde(default)]
    pub concurrency_limit: Option<i32>,
    #[serde(default)]
    pub retry_policy: RetryPolicy,
    #[serde(default)]
    pub triggers: TriggerConfig,
    pub steps: Vec<WorkflowStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    #[serde(default = "default_true")]
    pub api: bool,
    #[serde(default)]
    pub webhook: bool,
    #[serde(default)]
    pub cron: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for TriggerConfig {
    fn default() -> Self {
        Self {
            api: true,
            webhook: false,
            cron: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_initial_interval")]
    pub initial_interval_seconds: u64,
    #[serde(default = "default_backoff_multiplier")]
    pub backoff_multiplier: f64,
    #[serde(default = "default_max_interval")]
    pub max_interval_seconds: u64,
    #[serde(default = "default_jitter")]
    pub jitter_ratio: f64,
}

fn default_max_attempts() -> u32 {
    3
}
fn default_initial_interval() -> u64 {
    5
}
fn default_backoff_multiplier() -> f64 {
    2.0
}
fn default_max_interval() -> u64 {
    60
}
fn default_jitter() -> f64 {
    0.1
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            initial_interval_seconds: default_initial_interval(),
            backoff_multiplier: default_backoff_multiplier(),
            max_interval_seconds: default_max_interval(),
            jitter_ratio: default_jitter(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowStep {
    Http(HttpStep),
    #[serde(rename = "ai.openai")]
    AiOpenAi(AiStep),
    #[serde(rename = "db.postgres")]
    DbPostgres(DbStep),
}

impl WorkflowStep {
    pub fn name(&self) -> &str {
        match self {
            WorkflowStep::Http(step) => &step.name,
            WorkflowStep::AiOpenAi(step) => &step.name,
            WorkflowStep::DbPostgres(step) => &step.name,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpStep {
    pub name: String,
    #[serde(default = "default_method")]
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub mock_behavior: Option<MockBehavior>,
}

fn default_method() -> String {
    "POST".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiStep {
    pub name: String,
    pub prompt: String,
    #[serde(default)]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DbStep {
    pub name: String,
    #[serde(default = "default_table")]
    pub table: String,
    pub record: Value,
}

fn default_table() -> String {
    "artifacts".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MockBehavior {
    #[serde(default)]
    pub fail_until_attempt: u32,
    #[serde(default)]
    pub response: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunContext {
    #[serde(default)]
    pub input: Value,
    #[serde(default)]
    pub steps: Vec<StepContext>,
}

impl Default for RunContext {
    fn default() -> Self {
        Self {
            input: Value::Null,
            steps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepContext {
    pub name: String,
    pub output: Value,
    pub finished_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_slug: String,
    pub workflow_name: String,
    pub status: String,
    pub trigger_kind: String,
    pub current_step_index: i32,
    pub error: Option<String>,
    pub idempotency_key: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub dead_lettered: bool,
    pub replayed_from_run_id: Option<Uuid>,
    pub replay_run_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepAttemptSummary {
    pub id: Uuid,
    pub step_index: i32,
    pub step_name: String,
    pub status: String,
    pub attempt: i32,
    pub input: Value,
    pub output: Option<Value>,
    pub error: Option<String>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub next_retry_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunDetail {
    pub run: RunSummary,
    pub input: Value,
    pub context: Value,
    pub attempts: Vec<StepAttemptSummary>,
    pub dead_letter: Option<DeadLetterSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeadLetterSummary {
    pub id: Uuid,
    pub run_id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_slug: String,
    pub workflow_name: String,
    pub failed_step_index: i32,
    pub failed_step_name: String,
    pub terminal_error: String,
    pub last_attempt: i32,
    pub created_at: DateTime<Utc>,
    pub replay_run_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSummary {
    pub id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub webhook_token: String,
    pub has_published_version: bool,
    pub draft_definition: Value,
    pub published_definition: Option<Value>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowHistoryEntry {
    pub id: Uuid,
    pub version: i32,
    pub created_at: DateTime<Utc>,
    pub definition: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageSummary {
    pub workspace_name: String,
    pub plan: String,
    pub monthly_limit: i64,
    pub executions_this_month: i64,
    pub remaining: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateWorkflowRequest {
    pub slug: String,
    pub definition: WorkflowDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateWorkflowDraftRequest {
    pub definition: WorkflowDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerWorkflowRequest {
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerWorkflowResponse {
    pub run_id: Uuid,
    pub status: String,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunListFilters {
    pub status: Option<String>,
    pub workflow_id: Option<Uuid>,
    pub trigger_kind: Option<String>,
    pub dead_lettered: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunActionResponse {
    pub run_id: Uuid,
    pub status: String,
    pub related_run_id: Option<Uuid>,
    pub deduplicated: bool,
}
