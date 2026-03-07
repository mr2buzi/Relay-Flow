alter table workflow_runs
    add column if not exists replayed_from_run_id uuid references workflow_runs(id) on delete set null;

create unique index if not exists workflow_runs_replayed_from_unique
    on workflow_runs (replayed_from_run_id)
    where replayed_from_run_id is not null;

create table if not exists dead_letter_runs (
    id uuid primary key default gen_random_uuid(),
    run_id uuid not null unique references workflow_runs(id) on delete cascade,
    workflow_id uuid not null references workflows(id) on delete cascade,
    failed_step_index integer not null,
    failed_step_name text not null,
    terminal_error text not null,
    last_attempt integer not null,
    created_at timestamptz not null default now()
);
