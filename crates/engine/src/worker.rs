use crate::config::AppConfig;
use crate::db::{self, ClaimContext};
use crate::models::{
    BranchDecision, Condition, ConditionOperator, HttpStep, IfStep, RunContext, WorkflowDefinition,
    WorkflowStep,
};
use crate::templates::{render_json, render_string, resolve_reference};
use anyhow::{anyhow, Result};
use chrono::Utc;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde_json::{json, Value};
use tracing::{error, info};

pub async fn run_worker(config: AppConfig) -> Result<()> {
    let state = db::connect(&config).await?;
    info!("worker started");

    loop {
        process_schedules(&state).await?;
        if let Some(claim) = db::claim_next_run(&state.pool).await? {
            if let Err(error) = process_run(&state, claim).await {
                error!("run processing failed: {error}");
            }
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(750)).await;
        }
    }
}

async fn process_schedules(state: &db::AppState) -> Result<()> {
    for schedule in db::due_schedules(&state.pool).await? {
        let payload = json!({
            "scheduled_at": schedule.next_run_at,
            "source": "cron"
        });
        let key = format!(
            "cron:{}:{}",
            schedule.workflow_id,
            schedule.next_run_at.timestamp()
        );
        db::trigger_workflow(
            &state.pool,
            &schedule.workflow_slug,
            payload,
            Some(key),
            "cron".to_string(),
        )
        .await?;
        db::advance_schedule(&state.pool, &schedule).await?;
    }
    Ok(())
}

async fn process_run(state: &db::AppState, claim: ClaimContext) -> Result<()> {
    let definition: WorkflowDefinition = serde_json::from_value(claim.run.definition.clone())?;
    let mut context: RunContext = serde_json::from_value(claim.run.context.clone())?;
    let step_index = claim.run.current_step_index.max(0) as usize;
    let mut should_persist_context = false;

    if context.execution_plan.is_empty() {
        context.execution_plan = definition.steps.clone();
        should_persist_context = true;
    }

    let step = loop {
        let Some(step) = context.execution_plan.get(step_index).cloned() else {
            db::complete_run_without_step(&state.pool, claim.run.id, &context).await?;
            return Ok(());
        };

        match step {
            WorkflowStep::If(branch) => {
                resolve_branch(&mut context, step_index, branch);
                should_persist_context = true;
            }
            executable => break executable,
        }
    };

    if should_persist_context {
        db::save_run_state(&state.pool, claim.run.id, &context, step_index as i32).await?;
    }

    let input = build_step_input(&step, &context);
    let attempt = db::attempt_number(&state.pool, claim.run.id, step_index as i32).await?;
    let step_attempt_id = db::create_step_attempt(
        &state.pool,
        claim.run.id,
        step_index as i32,
        step.name(),
        attempt,
        &input,
    )
    .await?;
    let outcome = execute_step(state, &claim, &step, step_index as i32, attempt, &input).await;

    match outcome {
        Ok(output) => {
            if step_index + 1 == context.execution_plan.len() {
                db::complete_run_success(
                    &state.pool,
                    claim.run.id,
                    step_attempt_id,
                    step.name(),
                    output,
                )
                .await?;
            } else {
                db::complete_step_success(
                    &state.pool,
                    claim.run.id,
                    step_attempt_id,
                    step.name(),
                    output,
                    (step_index + 1) as i32,
                )
                .await?;
            }
        }
        Err(error) => {
            db::fail_step(
                &state.pool,
                &claim.run,
                step_attempt_id,
                step_index as i32,
                step.name(),
                attempt,
                &definition.retry_policy,
                &error.to_string(),
            )
            .await?;
        }
    }

    Ok(())
}

fn build_step_input(step: &WorkflowStep, context: &RunContext) -> Value {
    match step {
        WorkflowStep::Http(step) => json!({
            "method": step.method,
            "url": render_string(&step.url, context),
            "headers": step.headers.iter().map(|(k, v)| (k.clone(), Value::String(render_string(v, context)))).collect::<serde_json::Map<String, Value>>(),
            "body": step.body.as_ref().map(|value| render_json(value, context)).unwrap_or(Value::Null)
        }),
        WorkflowStep::AiOpenAi(step) => json!({
            "prompt": render_string(&step.prompt, context),
            "model": step.model.clone().unwrap_or_else(|| "gpt-4o-mini".to_string())
        }),
        WorkflowStep::DbPostgres(step) => json!({
            "table": step.table,
            "record": render_json(&step.record, context)
        }),
        WorkflowStep::If(_) => Value::Null,
    }
}

async fn execute_step(
    state: &db::AppState,
    claim: &ClaimContext,
    step: &WorkflowStep,
    step_index: i32,
    attempt: i32,
    input: &Value,
) -> Result<Value> {
    let idempotency_key = format!("{}:{}:{}", claim.run.id, step.name(), step_index);
    match step {
        WorkflowStep::Http(step) => execute_http_step(step, attempt, input, &idempotency_key).await,
        WorkflowStep::AiOpenAi(_) => execute_ai_step(state, input).await,
        WorkflowStep::DbPostgres(step) => {
            let record = input.get("record").cloned().unwrap_or(Value::Null);
            db::persist_artifact(
                &state.pool,
                claim.run.workflow_id,
                claim.run.id,
                step_index,
                &step.table,
                record,
            )
            .await
        }
        WorkflowStep::If(_) => Err(anyhow!("control steps cannot be executed directly")),
    }
}

fn resolve_branch(context: &mut RunContext, step_index: usize, branch: IfStep) {
    let matched = evaluate_condition(&branch.condition, context);
    let (chosen_branch, chosen_steps) = if matched {
        ("then".to_string(), branch.then_steps.clone())
    } else if branch.else_steps.is_empty() {
        ("skipped".to_string(), Vec::new())
    } else {
        ("else".to_string(), branch.else_steps.clone())
    };
    let inserted_steps = chosen_steps
        .iter()
        .map(|step| step.name().to_string())
        .collect::<Vec<_>>();

    context.branch_decisions.retain(|decision| {
        !(decision.step_index == step_index && decision.step_name == branch.name)
    });
    context.branch_decisions.push(BranchDecision {
        step_name: branch.name.clone(),
        step_index,
        condition: branch.condition.clone(),
        matched,
        chosen_branch,
        inserted_steps,
        evaluated_at: Utc::now(),
    });
    context
        .execution_plan
        .splice(step_index..=step_index, chosen_steps);
}

fn evaluate_condition(condition: &Condition, context: &RunContext) -> bool {
    let Some(actual) = resolve_reference(&condition.path, context) else {
        return matches!(condition.operator, ConditionOperator::Exists) && false;
    };

    match condition.operator {
        ConditionOperator::Exists => true,
        ConditionOperator::Equals => condition
            .value
            .as_ref()
            .is_some_and(|expected| actual == *expected),
        ConditionOperator::NotEquals => condition
            .value
            .as_ref()
            .is_some_and(|expected| actual != *expected),
        ConditionOperator::Contains => condition
            .value
            .as_ref()
            .is_some_and(|expected| contains_value(&actual, expected)),
        ConditionOperator::Gt => condition.value.as_ref().is_some_and(|expected| {
            compare_values(&actual, expected).is_some_and(|result| result.is_gt())
        }),
        ConditionOperator::Lt => condition.value.as_ref().is_some_and(|expected| {
            compare_values(&actual, expected).is_some_and(|result| result.is_lt())
        }),
    }
}

fn contains_value(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::String(actual), Value::String(expected)) => actual.contains(expected),
        (Value::Array(actual), _) => actual.iter().any(|item| item == expected),
        _ => false,
    }
}

fn compare_values(actual: &Value, expected: &Value) -> Option<std::cmp::Ordering> {
    match (actual, expected) {
        (Value::Number(actual), Value::Number(expected)) => {
            let actual = actual.as_f64()?;
            let expected = expected.as_f64()?;
            actual.partial_cmp(&expected)
        }
        (Value::String(actual), Value::String(expected)) => Some(actual.cmp(expected)),
        _ => None,
    }
}

async fn execute_http_step(
    step: &HttpStep,
    attempt: i32,
    input: &Value,
    idempotency_key: &str,
) -> Result<Value> {
    let url = input
        .get("url")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("missing url"))?;
    let body = input.get("body").cloned().unwrap_or(Value::Null);

    if url.starts_with("mock://") {
        if let Some(mock_behavior) = &step.mock_behavior {
            if attempt <= mock_behavior.fail_until_attempt as i32 {
                return Err(anyhow!("mock transport failure for {}", step.name));
            }
            if !mock_behavior.response.is_null() {
                return Ok(mock_behavior.response.clone());
            }
        }

        return Ok(match url {
            "mock://stripe/customers" => json!({
                "provider": "mock_stripe",
                "customer_id": format!("cus_{}", idempotency_key.replace(':', "_")),
                "status": "created",
                "request": body
            }),
            "mock://resend/send" => json!({
                "provider": "mock_email",
                "message_id": format!("msg_{}", idempotency_key.replace(':', "_")),
                "status": "sent",
                "request": body
            }),
            "mock://ocr/extract" => json!({
                "provider": "mock_ocr",
                "document_id": body.get("document_id").cloned().unwrap_or(Value::Null),
                "extracted_text": body.get("source_text").cloned().unwrap_or(Value::String("No source text".to_string()))
            }),
            "mock://scraper/page" => json!({
                "provider": "mock_scraper",
                "title": "RelayFlow reliability article",
                "content": format!("Scraped content for {}", body.get("url").and_then(Value::as_str).unwrap_or("unknown"))
            }),
            _ => json!({
                "provider": "mock_http",
                "url": url,
                "body": body
            }),
        });
    }

    let client = reqwest::Client::new();
    let mut headers = HeaderMap::new();
    headers.insert(
        "Idempotency-Key",
        HeaderValue::from_str(idempotency_key).map_err(|error| anyhow!(error.to_string()))?,
    );
    if let Some(configured_headers) = input.get("headers").and_then(Value::as_object) {
        for (name, value) in configured_headers {
            let header_name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| anyhow!(error.to_string()))?;
            let header_value = HeaderValue::from_str(value.as_str().unwrap_or(""))
                .map_err(|error| anyhow!(error.to_string()))?;
            headers.insert(header_name, header_value);
        }
    }
    let response = client
        .request(step.method.parse()?, url)
        .headers(headers)
        .json(&body)
        .send()
        .await?;
    let status = response.status();
    let body: Value = response
        .json()
        .await
        .unwrap_or(json!({ "status": status.as_u16() }));
    if status.is_success() {
        Ok(body)
    } else {
        Err(anyhow!("http step failed with status {}", status))
    }
}

async fn execute_ai_step(state: &db::AppState, input: &Value) -> Result<Value> {
    let prompt = input.get("prompt").and_then(Value::as_str).unwrap_or("");
    if let Some(api_key) = &state.config.openai_api_key {
        let client = reqwest::Client::new();
        let response = client
            .post("https://api.openai.com/v1/responses")
            .bearer_auth(api_key)
            .json(&json!({
                "model": input.get("model").and_then(Value::as_str).unwrap_or("gpt-4o-mini"),
                "input": prompt
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "openai call failed with status {}",
                response.status()
            ));
        }
        let body: Value = response.json().await?;
        let summary = body
            .get("output")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("content"))
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .and_then(|item| item.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("No summary returned")
            .to_string();
        Ok(json!({
            "provider": "openai",
            "summary": summary
        }))
    } else {
        let trimmed = prompt.replace('\n', " ");
        let short = trimmed.chars().take(120).collect::<String>();
        Ok(json!({
            "provider": "mock_openai",
            "summary": format!("Mock summary: {}", short),
            "mode": "mock"
        }))
    }
}
