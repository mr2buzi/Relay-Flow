use crate::config::AppConfig;
use crate::models::*;
use crate::seed::demo_workflows;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use rand::Rng;
use serde_json::{json, Value};
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{PgPool, Row, Transaction};
use uuid::Uuid;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

#[derive(Clone)]
pub struct AppState {
    pub pool: PgPool,
    pub config: AppConfig,
}

#[derive(Debug, Clone)]
pub struct WorkflowRow {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub slug: String,
    pub name: String,
    pub description: Option<String>,
    pub draft_definition: Value,
    pub published_definition: Option<Value>,
    pub published_version_id: Option<Uuid>,
    pub webhook_token: String,
}

#[derive(Debug, Clone)]
pub struct RunRow {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_version_id: Uuid,
    pub workflow_slug: String,
    pub workflow_name: String,
    pub status: String,
    pub trigger_kind: String,
    pub input: Value,
    pub context: Value,
    pub current_step_index: i32,
    pub idempotency_key: Option<String>,
    pub error: Option<String>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub definition: Value,
}

#[derive(Debug, Clone)]
pub struct DueSchedule {
    pub id: Uuid,
    pub workflow_id: Uuid,
    pub workflow_slug: String,
    pub workflow_version_id: Uuid,
    pub cron_expression: String,
    pub next_run_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct ClaimContext {
    pub run: RunRow,
    pub workflow: WorkflowRow,
    pub active_runs: i64,
    pub concurrency_limit: i32,
}

pub async fn connect(config: &AppConfig) -> Result<AppState> {
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await?;

    MIGRATOR.run(&pool).await?;
    ensure_seed_data(&pool, config).await?;

    Ok(AppState {
        pool,
        config: config.clone(),
    })
}

async fn ensure_seed_data(pool: &PgPool, config: &AppConfig) -> Result<()> {
    let workspace_id: Option<Uuid> = sqlx::query_scalar("select id from workspaces limit 1")
        .fetch_optional(pool)
        .await?;

    let workspace_id = if let Some(existing) = workspace_id {
        existing
    } else {
        sqlx::query_scalar(
            r#"
            insert into workspaces (name, plan, api_key)
            values ($1, $2, $3)
            returning id
            "#,
        )
        .bind("RelayFlow Demo")
        .bind("pro")
        .bind("demo_api_key")
        .fetch_one(pool)
        .await?
    };

    for (slug, definition) in demo_workflows() {
        let existing: Option<Uuid> = sqlx::query_scalar("select id from workflows where slug = $1")
            .bind(slug)
            .fetch_optional(pool)
            .await?;

        if existing.is_some() {
            continue;
        }

        let draft_definition = serde_json::to_value(&definition)?;
        let workflow_id: Uuid = sqlx::query_scalar(
            r#"
            insert into workflows (workspace_id, slug, name, description, draft_definition, webhook_token)
            values ($1, $2, $3, $4, $5, $6)
            returning id
            "#,
        )
        .bind(workspace_id)
        .bind(slug)
        .bind(&definition.name)
        .bind(&definition.description)
        .bind(&draft_definition)
        .bind(Uuid::new_v4().simple().to_string())
        .fetch_one(pool)
        .await?;

        publish_workflow(pool, workflow_id).await?;
    }

    if config.mode == "demo" {
        let runs: i64 = sqlx::query_scalar("select count(*) from workflow_runs")
            .fetch_one(pool)
            .await?;
        if runs == 0 {
            enqueue_demo_runs(pool).await?;
        }
    }

    Ok(())
}

async fn enqueue_demo_runs(pool: &PgPool) -> Result<()> {
    let workflow_slugs = vec![
        (
            "user-signup",
            json!({"user_id":"u_123","email":"demo@relayflow.dev","plan":"pro"}),
        ),
        (
            "document-summarize",
            json!({"document_id":"doc_001","source_text":"RelayFlow gives developers a reliable way to orchestrate APIs and AI steps with retries and observability."}),
        ),
        (
            "scrape-and-brief",
            json!({"url":"https://relayflow.dev/blog/reliability"}),
        ),
    ];

    for (slug, payload) in workflow_slugs {
        trigger_workflow(
            pool,
            slug,
            payload,
            Some(format!("seed-{slug}")),
            "api".to_string(),
        )
        .await?;
    }

    Ok(())
}

pub async fn list_workflows(pool: &PgPool) -> Result<Vec<WorkflowSummary>> {
    let rows = sqlx::query(
        r#"
        select
            w.id,
            w.slug,
            w.name,
            w.description,
            w.webhook_token,
            w.draft_definition,
            pv.definition as published_definition,
            w.updated_at,
            w.published_version_id is not null as has_published_version
        from workflows w
        left join workflow_versions pv on pv.id = w.published_version_id
        order by w.updated_at desc
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(WorkflowSummary {
                id: row.try_get("id")?,
                slug: row.try_get("slug")?,
                name: row.try_get("name")?,
                description: row.try_get("description")?,
                webhook_token: row.try_get("webhook_token")?,
                has_published_version: row.try_get("has_published_version")?,
                draft_definition: row.try_get("draft_definition")?,
                published_definition: row.try_get("published_definition")?,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .collect()
}

pub async fn create_workflow(
    pool: &PgPool,
    request: CreateWorkflowRequest,
) -> Result<WorkflowSummary> {
    validate_definition(&request.definition)?;
    let workspace_id: Uuid = sqlx::query_scalar("select id from workspaces limit 1")
        .fetch_one(pool)
        .await?;
    let draft_definition = serde_json::to_value(request.definition)?;
    let row = sqlx::query(
        r#"
        insert into workflows (workspace_id, slug, name, description, draft_definition, webhook_token)
        values ($1, $2, $3, $4, $5, $6)
        returning id, slug, name, description, webhook_token, draft_definition, updated_at
        "#,
    )
    .bind(workspace_id)
    .bind(request.slug)
    .bind(draft_definition.get("name").and_then(Value::as_str).unwrap_or("Untitled workflow"))
    .bind(draft_definition.get("description").and_then(Value::as_str))
    .bind(&draft_definition)
    .bind(Uuid::new_v4().simple().to_string())
    .fetch_one(pool)
    .await?;

    Ok(WorkflowSummary {
        id: row.try_get("id")?,
        slug: row.try_get("slug")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        webhook_token: row.try_get("webhook_token")?,
        has_published_version: false,
        draft_definition: row.try_get("draft_definition")?,
        published_definition: None,
        updated_at: row.try_get("updated_at")?,
    })
}

pub async fn update_workflow_draft(
    pool: &PgPool,
    workflow_id: Uuid,
    request: UpdateWorkflowDraftRequest,
) -> Result<WorkflowSummary> {
    validate_definition(&request.definition)?;
    let draft_definition = serde_json::to_value(request.definition.clone())?;
    let row = sqlx::query(
        r#"
        update workflows
        set name = $2,
            description = $3,
            draft_definition = $4,
            updated_at = now()
        where id = $1
        returning id, slug, name, description, webhook_token, draft_definition, updated_at, published_version_id
        "#,
    )
    .bind(workflow_id)
    .bind(&request.definition.name)
    .bind(&request.definition.description)
    .bind(&draft_definition)
    .fetch_one(pool)
    .await?;

    let published_version_id: Option<Uuid> = row.try_get("published_version_id")?;
    let published_definition = if let Some(version_id) = published_version_id {
        sqlx::query_scalar("select definition from workflow_versions where id = $1")
            .bind(version_id)
            .fetch_optional(pool)
            .await?
    } else {
        None
    };

    Ok(WorkflowSummary {
        id: row.try_get("id")?,
        slug: row.try_get("slug")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        webhook_token: row.try_get("webhook_token")?,
        has_published_version: published_version_id.is_some(),
        draft_definition: row.try_get("draft_definition")?,
        published_definition,
        updated_at: row.try_get("updated_at")?,
    })
}

pub async fn publish_workflow(pool: &PgPool, workflow_id: Uuid) -> Result<WorkflowHistoryEntry> {
    let mut tx = pool.begin().await?;
    let workflow = sqlx::query("select draft_definition from workflows where id = $1 for update")
        .bind(workflow_id)
        .fetch_one(&mut *tx)
        .await?;
    let definition: Value = workflow.try_get("draft_definition")?;
    let version: i32 = sqlx::query_scalar(
        "select coalesce(max(version), 0) + 1 from workflow_versions where workflow_id = $1",
    )
    .bind(workflow_id)
    .fetch_one(&mut *tx)
    .await?;
    let version_id: Uuid = sqlx::query_scalar(
        r#"
        insert into workflow_versions (workflow_id, version, definition)
        values ($1, $2, $3)
        returning id
        "#,
    )
    .bind(workflow_id)
    .bind(version)
    .bind(&definition)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query("update workflows set published_version_id = $2, updated_at = now() where id = $1")
        .bind(workflow_id)
        .bind(version_id)
        .execute(&mut *tx)
        .await?;

    sync_schedule(&mut tx, workflow_id, version_id, &definition).await?;
    tx.commit().await?;

    Ok(WorkflowHistoryEntry {
        id: version_id,
        version,
        created_at: Utc::now(),
        definition,
    })
}

async fn sync_schedule(
    tx: &mut Transaction<'_, sqlx::Postgres>,
    workflow_id: Uuid,
    version_id: Uuid,
    definition: &Value,
) -> Result<()> {
    let workflow_def: WorkflowDefinition = serde_json::from_value(definition.clone())?;
    if let Some(expression) = workflow_def.triggers.cron {
        let next_run_at = compute_next_run(&expression, Utc::now())?;
        sqlx::query(
            r#"
            insert into workflow_schedules (workflow_id, workflow_version_id, cron_expression, next_run_at)
            values ($1, $2, $3, $4)
            on conflict (workflow_id)
            do update set workflow_version_id = excluded.workflow_version_id,
                          cron_expression = excluded.cron_expression,
                          next_run_at = excluded.next_run_at,
                          enabled = true
            "#,
        )
        .bind(workflow_id)
        .bind(version_id)
        .bind(expression)
        .bind(next_run_at)
        .execute(&mut **tx)
        .await?;
    } else {
        sqlx::query("update workflow_schedules set enabled = false where workflow_id = $1")
            .bind(workflow_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

pub async fn workflow_history(
    pool: &PgPool,
    workflow_id: Uuid,
) -> Result<Vec<WorkflowHistoryEntry>> {
    let rows = sqlx::query(
        "select id, version, created_at, definition from workflow_versions where workflow_id = $1 order by version desc",
    )
    .bind(workflow_id)
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(WorkflowHistoryEntry {
                id: row.try_get("id")?,
                version: row.try_get("version")?,
                created_at: row.try_get("created_at")?,
                definition: row.try_get("definition")?,
            })
        })
        .collect()
}

pub async fn list_runs(pool: &PgPool) -> Result<Vec<RunSummary>> {
    let rows = sqlx::query(
        r#"
        select
            r.id,
            r.workflow_id,
            w.slug as workflow_slug,
            w.name as workflow_name,
            r.status,
            r.trigger_kind,
            r.current_step_index,
            r.error,
            r.idempotency_key,
            r.created_at,
            r.started_at,
            r.finished_at,
            r.next_retry_at
        from workflow_runs r
        join workflows w on w.id = r.workflow_id
        order by r.created_at desc
        limit 100
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.iter().map(run_summary_from_row).collect()
}

pub async fn get_run_detail(pool: &PgPool, run_id: Uuid) -> Result<RunDetail> {
    let row = sqlx::query(
        r#"
        select
            r.id,
            r.workflow_id,
            w.slug as workflow_slug,
            w.name as workflow_name,
            r.status,
            r.trigger_kind,
            r.current_step_index,
            r.error,
            r.idempotency_key,
            r.created_at,
            r.started_at,
            r.finished_at,
            r.next_retry_at,
            r.input,
            r.context
        from workflow_runs r
        join workflows w on w.id = r.workflow_id
        where r.id = $1
        "#,
    )
    .bind(run_id)
    .fetch_one(pool)
    .await?;

    let attempts = sqlx::query(
        r#"
        select id, step_index, step_name, status, attempt, input, output, error, started_at, finished_at, next_retry_at
        from step_attempts
        where run_id = $1
        order by step_index asc, attempt asc
        "#,
    )
    .bind(run_id)
    .fetch_all(pool)
    .await?;

    Ok(RunDetail {
        run: run_summary_from_row(&row)?,
        input: row.try_get("input")?,
        context: row.try_get("context")?,
        attempts: attempts
            .into_iter()
            .map(|attempt| {
                Ok(StepAttemptSummary {
                    id: attempt.try_get("id")?,
                    step_index: attempt.try_get("step_index")?,
                    step_name: attempt.try_get("step_name")?,
                    status: attempt.try_get("status")?,
                    attempt: attempt.try_get("attempt")?,
                    input: attempt.try_get("input")?,
                    output: attempt.try_get("output")?,
                    error: attempt.try_get("error")?,
                    started_at: attempt.try_get("started_at")?,
                    finished_at: attempt.try_get("finished_at")?,
                    next_retry_at: attempt.try_get("next_retry_at")?,
                })
            })
            .collect::<Result<Vec<_>>>()?,
    })
}

fn run_summary_from_row(row: &PgRow) -> Result<RunSummary> {
    Ok(RunSummary {
        id: row.try_get("id")?,
        workflow_id: row.try_get("workflow_id")?,
        workflow_slug: row.try_get("workflow_slug")?,
        workflow_name: row.try_get("workflow_name")?,
        status: row.try_get("status")?,
        trigger_kind: row.try_get("trigger_kind")?,
        current_step_index: row.try_get("current_step_index")?,
        error: row.try_get("error")?,
        idempotency_key: row.try_get("idempotency_key")?,
        created_at: row.try_get("created_at")?,
        started_at: row.try_get("started_at")?,
        finished_at: row.try_get("finished_at")?,
        next_retry_at: row.try_get("next_retry_at")?,
    })
}

pub async fn usage_summary(pool: &PgPool) -> Result<UsageSummary> {
    let workspace = sqlx::query("select name, plan from workspaces limit 1")
        .fetch_one(pool)
        .await?;
    let plan: String = workspace.try_get("plan")?;
    let monthly_limit = plan_limit(&plan);
    let now = Utc::now();
    let start_of_month = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .ok_or_else(|| anyhow!("invalid month boundary"))?;
    let executions: i64 =
        sqlx::query_scalar("select count(*) from workflow_runs where created_at >= $1")
            .bind(start_of_month)
            .fetch_one(pool)
            .await?;
    Ok(UsageSummary {
        workspace_name: workspace.try_get("name")?,
        plan,
        monthly_limit,
        executions_this_month: executions,
        remaining: (monthly_limit - executions).max(0),
    })
}

pub async fn trigger_workflow(
    pool: &PgPool,
    slug: &str,
    payload: Value,
    idempotency_key: Option<String>,
    trigger_kind: String,
) -> Result<TriggerWorkflowResponse> {
    let usage = usage_summary(pool).await?;
    if usage.executions_this_month >= usage.monthly_limit {
        return Err(anyhow!(
            "monthly execution limit reached for plan {}",
            usage.plan
        ));
    }

    let workflow = sqlx::query(
        r#"
        select
            w.id,
            pv.id as published_version_id
        from workflows w
        join workflow_versions pv on pv.id = w.published_version_id
        where w.slug = $1
        "#,
    )
    .bind(slug)
    .fetch_one(pool)
    .await?;

    let workflow_id: Uuid = workflow.try_get("id")?;
    if let Some(key) = idempotency_key.clone() {
        if let Some(existing_run_id) = sqlx::query_scalar::<_, Uuid>(
            "select id from workflow_runs where workflow_id = $1 and idempotency_key = $2",
        )
        .bind(workflow_id)
        .bind(&key)
        .fetch_optional(pool)
        .await?
        {
            let status: String =
                sqlx::query_scalar("select status from workflow_runs where id = $1")
                    .bind(existing_run_id)
                    .fetch_one(pool)
                    .await?;
            return Ok(TriggerWorkflowResponse {
                run_id: existing_run_id,
                status,
                deduplicated: true,
            });
        }
    }

    let published_version_id: Uuid = workflow.try_get("published_version_id")?;
    let context = serde_json::to_value(RunContext {
        input: payload.clone(),
        steps: Vec::new(),
    })?;
    let run_id: Uuid = sqlx::query_scalar(
        r#"
        insert into workflow_runs
        (workflow_id, workflow_version_id, trigger_kind, status, input, context, current_step_index, idempotency_key, next_retry_at)
        values ($1, $2, $3, 'queued', $4, $5, 0, $6, now())
        returning id
        "#,
    )
    .bind(workflow_id)
    .bind(published_version_id)
    .bind(trigger_kind)
    .bind(payload)
    .bind(context)
    .bind(idempotency_key)
    .fetch_one(pool)
    .await?;

    Ok(TriggerWorkflowResponse {
        run_id,
        status: "queued".to_string(),
        deduplicated: false,
    })
}

pub async fn trigger_webhook(
    pool: &PgPool,
    token: &str,
    payload: Value,
) -> Result<TriggerWorkflowResponse> {
    let slug: String = sqlx::query_scalar("select slug from workflows where webhook_token = $1")
        .bind(token)
        .fetch_one(pool)
        .await?;
    trigger_workflow(pool, &slug, payload, None, "webhook".to_string()).await
}

pub async fn validate_api_key(pool: &PgPool, key: &str) -> Result<bool> {
    let exists: Option<Uuid> = sqlx::query_scalar("select id from workspaces where api_key = $1")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(exists.is_some())
}

pub async fn claim_next_run(pool: &PgPool) -> Result<Option<ClaimContext>> {
    let mut tx = pool.begin().await?;
    let row = sqlx::query(
        r#"
        select
            r.id,
            r.workflow_id,
            r.workflow_version_id,
            w.slug as workflow_slug,
            w.name as workflow_name,
            r.status,
            r.trigger_kind,
            r.input,
            r.context,
            r.current_step_index,
            r.idempotency_key,
            r.error,
            r.next_retry_at,
            r.started_at,
            r.finished_at,
            r.created_at,
            v.definition,
            w.workspace_id,
            w.description,
            w.draft_definition,
            w.webhook_token,
            pv.definition as published_definition,
            w.published_version_id
        from workflow_runs r
        join workflows w on w.id = r.workflow_id
        join workflow_versions v on v.id = r.workflow_version_id
        left join workflow_versions pv on pv.id = w.published_version_id
        where r.status in ('queued', 'retrying')
          and coalesce(r.next_retry_at, now()) <= now()
        order by r.created_at asc
        for update skip locked
        limit 1
        "#,
    )
    .fetch_optional(&mut *tx)
    .await?;

    let Some(row) = row else {
        tx.commit().await?;
        return Ok(None);
    };

    let workflow = WorkflowRow {
        id: row.try_get("workflow_id")?,
        workspace_id: row.try_get("workspace_id")?,
        slug: row.try_get("workflow_slug")?,
        name: row.try_get("workflow_name")?,
        description: row.try_get("description")?,
        draft_definition: row.try_get("draft_definition")?,
        published_definition: row.try_get("published_definition")?,
        published_version_id: row.try_get("published_version_id")?,
        webhook_token: row.try_get("webhook_token")?,
    };

    let definition: WorkflowDefinition = serde_json::from_value(row.try_get("definition")?)?;
    let concurrency_limit = definition.concurrency_limit.unwrap_or(2);
    let active_runs: i64 = sqlx::query_scalar(
        "select count(*) from workflow_runs where workflow_id = $1 and status = 'running'",
    )
    .bind(workflow.id)
    .fetch_one(&mut *tx)
    .await?;

    if active_runs >= concurrency_limit as i64 {
        sqlx::query(
            "update workflow_runs set next_retry_at = now() + interval '2 seconds' where id = $1",
        )
        .bind(row.try_get::<Uuid, _>("id")?)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        return Ok(None);
    }

    sqlx::query(
        "update workflow_runs set status = 'running', started_at = coalesce(started_at, now()), updated_at = now() where id = $1",
    )
    .bind(row.try_get::<Uuid, _>("id")?)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Some(ClaimContext {
        run: RunRow {
            id: row.try_get("id")?,
            workflow_id: row.try_get("workflow_id")?,
            workflow_version_id: row.try_get("workflow_version_id")?,
            workflow_slug: row.try_get("workflow_slug")?,
            workflow_name: row.try_get("workflow_name")?,
            status: row.try_get("status")?,
            trigger_kind: row.try_get("trigger_kind")?,
            input: row.try_get("input")?,
            context: row.try_get("context")?,
            current_step_index: row.try_get("current_step_index")?,
            idempotency_key: row.try_get("idempotency_key")?,
            error: row.try_get("error")?,
            next_retry_at: row.try_get("next_retry_at")?,
            started_at: row.try_get("started_at")?,
            finished_at: row.try_get("finished_at")?,
            created_at: row.try_get("created_at")?,
            definition: row.try_get("definition")?,
        },
        workflow,
        active_runs,
        concurrency_limit,
    }))
}

pub async fn attempt_number(pool: &PgPool, run_id: Uuid, step_index: i32) -> Result<i32> {
    let attempt: i64 = sqlx::query_scalar(
        "select count(*) from step_attempts where run_id = $1 and step_index = $2",
    )
    .bind(run_id)
    .bind(step_index)
    .fetch_one(pool)
    .await?;
    Ok((attempt + 1) as i32)
}

pub async fn create_step_attempt(
    pool: &PgPool,
    run_id: Uuid,
    step_index: i32,
    step_name: &str,
    attempt: i32,
    input: &Value,
) -> Result<Uuid> {
    let id = sqlx::query_scalar(
        r#"
        insert into step_attempts (run_id, step_index, step_name, status, attempt, input, started_at)
        values ($1, $2, $3, 'running', $4, $5, now())
        returning id
        "#,
    )
    .bind(run_id)
    .bind(step_index)
    .bind(step_name)
    .bind(attempt)
    .bind(input)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn complete_step_success(
    pool: &PgPool,
    run_id: Uuid,
    step_attempt_id: Uuid,
    step_name: &str,
    output: Value,
    next_step_index: i32,
) -> Result<()> {
    let row = sqlx::query("select context from workflow_runs where id = $1")
        .bind(run_id)
        .fetch_one(pool)
        .await?;
    let context_value: Value = row.try_get("context")?;
    let mut context: RunContext = serde_json::from_value(context_value)?;
    context.steps.push(StepContext {
        name: step_name.to_string(),
        output: output.clone(),
        finished_at: Utc::now(),
    });
    let context = serde_json::to_value(context)?;

    sqlx::query("update step_attempts set status = 'succeeded', output = $2, finished_at = now() where id = $1")
        .bind(step_attempt_id)
        .bind(&output)
        .execute(pool)
        .await?;

    sqlx::query(
        "update workflow_runs set context = $2, current_step_index = $3, status = 'queued', next_retry_at = now(), error = null, updated_at = now() where id = $1",
    )
    .bind(run_id)
    .bind(context)
    .bind(next_step_index)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn complete_run_success(
    pool: &PgPool,
    run_id: Uuid,
    step_attempt_id: Uuid,
    step_name: &str,
    output: Value,
) -> Result<()> {
    let row = sqlx::query("select context from workflow_runs where id = $1")
        .bind(run_id)
        .fetch_one(pool)
        .await?;
    let context_value: Value = row.try_get("context")?;
    let mut context: RunContext = serde_json::from_value(context_value)?;
    context.steps.push(StepContext {
        name: step_name.to_string(),
        output: output.clone(),
        finished_at: Utc::now(),
    });
    let context = serde_json::to_value(context)?;

    sqlx::query("update step_attempts set status = 'succeeded', output = $2, finished_at = now() where id = $1")
        .bind(step_attempt_id)
        .bind(&output)
        .execute(pool)
        .await?;

    sqlx::query(
        "update workflow_runs set context = $2, status = 'succeeded', finished_at = now(), next_retry_at = null, updated_at = now() where id = $1",
    )
    .bind(run_id)
    .bind(context)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn fail_step(
    pool: &PgPool,
    run: &RunRow,
    step_attempt_id: Uuid,
    attempt: i32,
    retry_policy: &RetryPolicy,
    error: &str,
) -> Result<()> {
    let should_retry = attempt < retry_policy.max_attempts as i32;
    let next_retry_at = if should_retry {
        Some(calculate_backoff(retry_policy, attempt))
    } else {
        None
    };

    sqlx::query(
        "update step_attempts set status = 'failed', error = $2, finished_at = now(), next_retry_at = $3 where id = $1",
    )
    .bind(step_attempt_id)
    .bind(error)
    .bind(next_retry_at)
    .execute(pool)
    .await?;

    if let Some(retry_at) = next_retry_at {
        sqlx::query(
            "update workflow_runs set status = 'retrying', next_retry_at = $2, error = $3, updated_at = now() where id = $1",
        )
        .bind(run.id)
        .bind(retry_at)
        .bind(error)
        .execute(pool)
        .await?;
    } else {
        sqlx::query(
            "update workflow_runs set status = 'failed', finished_at = now(), next_retry_at = null, error = $2, updated_at = now() where id = $1",
        )
        .bind(run.id)
        .bind(error)
        .execute(pool)
        .await?;
    }

    Ok(())
}

fn calculate_backoff(policy: &RetryPolicy, attempt: i32) -> DateTime<Utc> {
    let base = policy.initial_interval_seconds as f64
        * policy.backoff_multiplier.powi(attempt.saturating_sub(1));
    let capped = base.min(policy.max_interval_seconds as f64);
    let jitter_window = capped * policy.jitter_ratio;
    let jitter = if jitter_window > 0.0 {
        rand::thread_rng().gen_range(-jitter_window..=jitter_window)
    } else {
        0.0
    };
    Utc::now() + Duration::milliseconds(((capped + jitter).max(1.0) * 1000.0) as i64)
}

pub async fn persist_artifact(
    pool: &PgPool,
    workflow_id: Uuid,
    run_id: Uuid,
    step_index: i32,
    table: &str,
    record: Value,
) -> Result<Value> {
    let id: Uuid = sqlx::query_scalar(
        r#"
        insert into artifacts (workflow_id, run_id, step_index, artifact_table, record)
        values ($1, $2, $3, $4, $5)
        on conflict (run_id, step_index)
        do update set record = excluded.record, updated_at = now()
        returning id
        "#,
    )
    .bind(workflow_id)
    .bind(run_id)
    .bind(step_index)
    .bind(table)
    .bind(&record)
    .fetch_one(pool)
    .await?;
    Ok(json!({
        "artifact_id": id,
        "table": table,
        "record": record
    }))
}

pub async fn due_schedules(pool: &PgPool) -> Result<Vec<DueSchedule>> {
    let rows = sqlx::query(
        r#"
        select s.id, s.workflow_id, w.slug as workflow_slug, s.workflow_version_id, s.cron_expression, s.next_run_at
        from workflow_schedules s
        join workflows w on w.id = s.workflow_id
        where s.enabled = true and s.next_run_at <= now()
        order by s.next_run_at asc
        "#,
    )
    .fetch_all(pool)
    .await?;

    rows.into_iter()
        .map(|row| {
            Ok(DueSchedule {
                id: row.try_get("id")?,
                workflow_id: row.try_get("workflow_id")?,
                workflow_slug: row.try_get("workflow_slug")?,
                workflow_version_id: row.try_get("workflow_version_id")?,
                cron_expression: row.try_get("cron_expression")?,
                next_run_at: row.try_get("next_run_at")?,
            })
        })
        .collect()
}

pub async fn advance_schedule(pool: &PgPool, schedule: &DueSchedule) -> Result<()> {
    let next_run_at = compute_next_run(
        &schedule.cron_expression,
        schedule.next_run_at + Duration::seconds(1),
    )?;
    sqlx::query(
        "update workflow_schedules set last_run_at = now(), next_run_at = $2 where id = $1",
    )
    .bind(schedule.id)
    .bind(next_run_at)
    .execute(pool)
    .await?;
    Ok(())
}

fn compute_next_run(expression: &str, from: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let next = if expression == "0/30 * * * * * *" {
        truncate_to_second(from) + Duration::seconds(30)
    } else if expression == "15 * * * * * *" {
        let base = truncate_to_second(from);
        if base.second() < 15 {
            base.with_second(15)
                .ok_or_else(|| anyhow!("invalid cron second"))?
        } else {
            (base + Duration::minutes(1))
                .with_second(15)
                .ok_or_else(|| anyhow!("invalid cron second"))?
        }
    } else {
        truncate_to_second(from) + Duration::minutes(5)
    };
    Ok(next)
}

fn truncate_to_second(value: DateTime<Utc>) -> DateTime<Utc> {
    value.with_nanosecond(0).unwrap_or(value)
}

fn validate_definition(definition: &WorkflowDefinition) -> Result<()> {
    if definition.steps.is_empty() {
        return Err(anyhow!("workflow must include at least one step"));
    }
    for step in &definition.steps {
        if step.name().trim().is_empty() {
            return Err(anyhow!("workflow steps must have non-empty names"));
        }
    }
    Ok(())
}

fn plan_limit(plan: &str) -> i64 {
    match plan {
        "free" => 1_000,
        "team" => 1_000_000,
        _ => 100_000,
    }
}
