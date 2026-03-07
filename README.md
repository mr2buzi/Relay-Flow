# Relay-Flow

Relay-Flow is the strongest interview project in this repo because it shows backend and infrastructure judgment, not just feature building.

I built it as a developer-first execution engine for AI and API workflows with reliability guarantees:

- retries with backoff
- idempotent triggering
- durable step history
- dead-letter recovery
- conditional branching
- a local zero-secrets demo path

The core idea is simple: calling external APIs is easy, but making those workflows reliable is the hard part.

## Why This Project Works

This project signals the kind of work I want to be hired for:

- backend/platform/reliability/devtools roles
- systems thinking instead of CRUD
- clear tradeoffs instead of overengineering

The best example is v1.3 branching. I added `if` steps, but I intentionally did not jump to a full DAG scheduler. Instead, I persist an execution plan in run context so retries and replay stay deterministic. That is a strong design tradeoff to talk through in interviews.

## What It Does

Relay-Flow lets me define a workflow like this:

```json
{
  "workflow": "user_signup",
  "steps": [
    { "type": "http", "url": "mock://stripe/customers" },
    { "type": "http", "url": "mock://resend/send" },
    { "type": "if" },
    { "type": "db.postgres" }
  ]
}
```

The system executes steps sequentially, persists state after each transition, retries failures automatically, and records everything needed for debugging and recovery.

## Stack

- Rust
- Axum
- SQLx
- PostgreSQL
- React
- Vite
- Docker Compose

Repo layout:

- `apps/api`: API service
- `apps/worker`: execution worker
- `apps/dashboard`: React dashboard
- `crates/engine`: shared engine, models, persistence, worker logic, migrations, seed data

## Current Scope

Relay-Flow is intentionally narrow and opinionated.

- v1.0: workflow definitions, sequential execution, retries, idempotency, observability
- v1.2: dead letters, replay, retry-now controls, run filtering
- v1.3: conditional branching with persisted branch decisions and workflow preview

What I intentionally did not build yet:

- full DAG execution
- fan-out / fan-in
- multi-tenant auth
- live billing
- a connector marketplace

## Demo Workflows

- `user-signup`
  - good for showing branching and normal execution
- `document-summarize`
  - good for showing a clean AI pipeline
- `scrape-and-brief`
  - good for showing retries, failure, dead-lettering, and replay

## Local Run

Prerequisites:

- Docker Desktop
- Rust toolchain
- Node.js 22+
- npm

Fastest path:

```bash
docker compose up --build
```

Endpoints:

- Dashboard: `http://localhost:5173`
- API: `http://localhost:8000`
- Postgres: `localhost:5432`

Demo credentials:

- API key: `demo_api_key`
- Workspace: `RelayFlow Demo`

If I want to run services separately:

```bash
cargo run -p workflow-api
cargo run -p workflow-worker
cd apps/dashboard && npm install && npm run dev
```

## Interview Demo Script

If I only had 3 to 5 minutes, this is the flow I would use:

1. Start the stack with `docker compose up --build`.
2. Open the dashboard and use the `Quick Start` section.
3. Run `user-signup` to show:
   - published workflow versions
   - sequential durable execution
   - stored run context
   - persisted branch decisions
4. Run `scrape-and-brief` to show:
   - failure on first attempt
   - retry scheduling
   - dead-letter creation
   - replay as a new run

If I only showed one thing, I would use `scrape-and-brief` for reliability and `user-signup` for branching.

## What I Would Say In An Interview

Short version:

> I built Relay-Flow as a developer-first execution engine for AI and API workflows. The interesting problem is not calling APIs, it is making those workflows reliable. So I focused on published workflow versions, durable sequential execution, retries with backoff, idempotency, dead-letter recovery, and run history. In v1.3 I added branching, but I deliberately kept the runtime simple by persisting an execution plan instead of overengineering into a DAG scheduler.

Topics I would talk through:

- why runs bind to published workflow versions
- why the execution model is at-least-once
- where idempotency is enforced
- why replay creates a new run instead of mutating history
- why branch decisions live in run context
- why the repo is mock-first for a public GitHub project

## API Surface

Main endpoints:

- `POST /v1/workflows`
- `PUT /v1/workflows/:id/draft`
- `POST /v1/workflows/:id/publish`
- `POST /v1/workflows/:slug/run`
- `POST /v1/webhooks/:token`
- `GET /v1/workflows/:id/history`

Observability and recovery:

- `GET /v1/runs`
- `GET /v1/runs/:id`
- `GET /v1/dead-letters`
- `POST /v1/runs/:id/retry-now`
- `POST /v1/runs/:id/replay`
- `GET /v1/usage`

Run filters:

- `status`
- `workflow_id`
- `trigger_kind`
- `dead_lettered`

## Verification

I verified the repo with:

```bash
cargo check
cargo test
cd apps/dashboard && npm run build
```

## Tradeoffs

The tradeoffs are deliberate:

- sequential execution over DAG complexity
- branching without a full graph scheduler
- mock-first integrations over secret-heavy live connectors
- local operational clarity over multi-tenant production scope

That is exactly why this project works well in interviews: it shows scope control, not just ambition.
