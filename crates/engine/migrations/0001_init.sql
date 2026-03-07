create extension if not exists "pgcrypto";

create table if not exists workspaces (
    id uuid primary key default gen_random_uuid(),
    name text not null,
    plan text not null default 'pro',
    api_key text not null unique,
    created_at timestamptz not null default now()
);

create table if not exists workflows (
    id uuid primary key default gen_random_uuid(),
    workspace_id uuid not null references workspaces(id) on delete cascade,
    slug text not null unique,
    name text not null,
    description text,
    draft_definition jsonb not null,
    published_version_id uuid,
    webhook_token text not null unique,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create table if not exists workflow_versions (
    id uuid primary key default gen_random_uuid(),
    workflow_id uuid not null references workflows(id) on delete cascade,
    version integer not null,
    definition jsonb not null,
    created_at timestamptz not null default now(),
    unique (workflow_id, version)
);

alter table workflows
    add constraint workflows_published_version_fk
    foreign key (published_version_id)
    references workflow_versions(id)
    on delete set null;

create table if not exists workflow_runs (
    id uuid primary key default gen_random_uuid(),
    workflow_id uuid not null references workflows(id) on delete cascade,
    workflow_version_id uuid not null references workflow_versions(id) on delete cascade,
    trigger_kind text not null,
    status text not null,
    input jsonb not null default '{}'::jsonb,
    context jsonb not null default '{"input":{},"steps":[]}'::jsonb,
    current_step_index integer not null default 0,
    idempotency_key text,
    error text,
    next_retry_at timestamptz,
    started_at timestamptz,
    finished_at timestamptz,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now()
);

create unique index if not exists workflow_runs_idempotency_unique
    on workflow_runs (workflow_id, idempotency_key)
    where idempotency_key is not null;

create table if not exists step_attempts (
    id uuid primary key default gen_random_uuid(),
    run_id uuid not null references workflow_runs(id) on delete cascade,
    step_index integer not null,
    step_name text not null,
    status text not null,
    attempt integer not null,
    input jsonb not null default '{}'::jsonb,
    output jsonb,
    error text,
    started_at timestamptz not null default now(),
    finished_at timestamptz,
    next_retry_at timestamptz
);

create table if not exists workflow_schedules (
    id uuid primary key default gen_random_uuid(),
    workflow_id uuid not null unique references workflows(id) on delete cascade,
    workflow_version_id uuid not null references workflow_versions(id) on delete cascade,
    cron_expression text not null,
    next_run_at timestamptz not null,
    last_run_at timestamptz,
    enabled boolean not null default true
);

create table if not exists artifacts (
    id uuid primary key default gen_random_uuid(),
    workflow_id uuid not null references workflows(id) on delete cascade,
    run_id uuid not null references workflow_runs(id) on delete cascade,
    step_index integer not null,
    artifact_table text not null,
    record jsonb not null,
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    unique (run_id, step_index)
);
