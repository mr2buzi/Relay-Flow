use crate::models::{
    AiStep, Condition, ConditionOperator, DbStep, HttpStep, IfStep, MockBehavior, RetryPolicy,
    TriggerConfig, WorkflowDefinition, WorkflowStep,
};
use serde_json::json;

pub fn demo_workflows() -> Vec<(&'static str, WorkflowDefinition)> {
    vec![
        (
            "user-signup",
            WorkflowDefinition {
                name: "User signup onboarding".to_string(),
                description: Some("Mock Stripe + email + AI summary + durable storage.".to_string()),
                concurrency_limit: Some(4),
                retry_policy: RetryPolicy {
                    max_attempts: 3,
                    initial_interval_seconds: 5,
                    backoff_multiplier: 2.0,
                    max_interval_seconds: 45,
                    jitter_ratio: 0.15,
                },
                triggers: TriggerConfig {
                    api: true,
                    webhook: true,
                    cron: None,
                },
                steps: vec![
                    WorkflowStep::Http(HttpStep {
                        name: "create-billing-customer".to_string(),
                        method: "POST".to_string(),
                        url: "mock://stripe/customers".to_string(),
                        headers: Default::default(),
                        body: Some(json!({
                            "user_id": "{{input.user_id}}",
                            "email": "{{input.email}}",
                            "plan": "{{input.plan}}"
                        })),
                        mock_behavior: None,
                    }),
                    WorkflowStep::Http(HttpStep {
                        name: "send-welcome-email".to_string(),
                        method: "POST".to_string(),
                        url: "mock://resend/send".to_string(),
                        headers: Default::default(),
                        body: Some(json!({
                            "user_id": "{{input.user_id}}",
                            "customer_id": "{{steps.0.output.customer_id}}",
                            "email": "{{input.email}}"
                        })),
                        mock_behavior: None,
                    }),
                    WorkflowStep::If(IfStep {
                        name: "branch-on-plan".to_string(),
                        condition: Condition {
                            path: "input.plan".to_string(),
                            operator: ConditionOperator::Equals,
                            value: Some(json!("pro")),
                        },
                        then_steps: vec![
                            WorkflowStep::AiOpenAi(AiStep {
                                name: "generate-summary".to_string(),
                                prompt: "Summarize the onboarding event for {{input.email}} using customer {{steps.0.output.customer_id}}.".to_string(),
                                model: Some("gpt-4o-mini".to_string()),
                            }),
                            WorkflowStep::DbPostgres(DbStep {
                                name: "store-pro-artifact".to_string(),
                                table: "artifacts".to_string(),
                                record: json!({
                                    "kind": "signup_summary",
                                    "user_id": "{{input.user_id}}",
                                    "email": "{{input.email}}",
                                    "plan": "{{input.plan}}",
                                    "summary": "{{steps.2.output.summary}}",
                                    "customer_id": "{{steps.0.output.customer_id}}"
                                }),
                            }),
                        ],
                        else_steps: vec![WorkflowStep::DbPostgres(DbStep {
                            name: "store-standard-artifact".to_string(),
                            table: "artifacts".to_string(),
                            record: json!({
                                "kind": "signup_summary",
                                "user_id": "{{input.user_id}}",
                                "email": "{{input.email}}",
                                "plan": "{{input.plan}}",
                                "summary": "AI summary skipped for non-pro signup",
                                "customer_id": "{{steps.0.output.customer_id}}"
                            }),
                        })],
                    }),
                ],
            },
        ),
        (
            "document-summarize",
            WorkflowDefinition {
                name: "Document summarize".to_string(),
                description: Some("Webhook or cron trigger for OCR-style summarization.".to_string()),
                concurrency_limit: Some(2),
                retry_policy: RetryPolicy::default(),
                triggers: TriggerConfig {
                    api: true,
                    webhook: true,
                    cron: Some("0/30 * * * * * *".to_string()),
                },
                steps: vec![
                    WorkflowStep::Http(HttpStep {
                        name: "mock-ocr".to_string(),
                        method: "POST".to_string(),
                        url: "mock://ocr/extract".to_string(),
                        headers: Default::default(),
                        body: Some(json!({
                            "document_id": "{{input.document_id}}",
                            "source_text": "{{input.source_text}}"
                        })),
                        mock_behavior: None,
                    }),
                    WorkflowStep::AiOpenAi(AiStep {
                        name: "summarize-document".to_string(),
                        prompt: "Create a concise summary for: {{steps.0.output.extracted_text}}".to_string(),
                        model: Some("gpt-4o-mini".to_string()),
                    }),
                    WorkflowStep::DbPostgres(DbStep {
                        name: "store-document-summary".to_string(),
                        table: "artifacts".to_string(),
                        record: json!({
                            "kind": "document_summary",
                            "document_id": "{{input.document_id}}",
                            "summary": "{{steps.1.output.summary}}"
                        }),
                    }),
                ],
            },
        ),
        (
            "scrape-and-brief",
            WorkflowDefinition {
                name: "Scrape and brief".to_string(),
                description: Some("Demonstrates retries by intentionally failing the first scrape attempt.".to_string()),
                concurrency_limit: Some(1),
                retry_policy: RetryPolicy {
                    max_attempts: 4,
                    initial_interval_seconds: 3,
                    backoff_multiplier: 2.0,
                    max_interval_seconds: 30,
                    jitter_ratio: 0.0,
                },
                triggers: TriggerConfig {
                    api: true,
                    webhook: false,
                    cron: Some("15 * * * * * *".to_string()),
                },
                steps: vec![
                    WorkflowStep::Http(HttpStep {
                        name: "scrape-page".to_string(),
                        method: "GET".to_string(),
                        url: "mock://scraper/page".to_string(),
                        headers: Default::default(),
                        body: Some(json!({
                            "url": "{{input.url}}"
                        })),
                        mock_behavior: Some(MockBehavior {
                            fail_until_attempt: 1,
                            response: serde_json::Value::Null,
                        }),
                    }),
                    WorkflowStep::AiOpenAi(AiStep {
                        name: "brief-page".to_string(),
                        prompt: "Write a short engineering brief for this page: {{steps.0.output.content}}".to_string(),
                        model: Some("gpt-4o-mini".to_string()),
                    }),
                    WorkflowStep::DbPostgres(DbStep {
                        name: "store-brief".to_string(),
                        table: "artifacts".to_string(),
                        record: json!({
                            "kind": "scrape_brief",
                            "url": "{{input.url}}",
                            "title": "{{steps.0.output.title}}",
                            "summary": "{{steps.1.output.summary}}"
                        }),
                    }),
                ],
            },
        ),
    ]
}
