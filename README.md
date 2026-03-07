# Relay-Flow

I built Relay-Flow as an interview project to show that I can design and ship backend infrastructure, not just CRUD apps. The product is a developer-first execution engine for AI and API workflows with reliability guarantees: retries, backoff, idempotency, observability, job history, dead-letter recovery flows, and now conditional branching.

Instead of trying to compete with visual automation tools like Zapier, Make, or n8n on UI alone, I focused this project on the engineering problem behind workflow automation: external APIs fail all the time, and production systems need a durable way to recover safely.

## What This Project Does

With Relay-Flow, I can define workflows such as:

```json
{
  "workflow": "user_signup",
  "steps": [
    { "type": "http", "url": "mock://stripe/customers" },
    { "type": "http", "url": "mock://resend/send" },
    { "type": "ai.openai" },
    { "type": "db.postgres" }
  ]
}
```

The engine executes those steps sequentially and persists state between each transition. If a step fails, the system records the attempt, calculates the next retry time, retries automatically, and if retries are exhausted it creates a dead-letter record that can be inspected and replayed from the dashboard. In v1.3, the engine can also evaluate `if` steps, choose a branch based on structured conditions, and persist that branch decision in run context for replay-safe execution.

## Why I Built It This Way

I wanted this project to show five things clearly in interviews:

1. I can build backend systems with real operational concerns.
2. I understand workflow state, retries, idempotency, and failure recovery.
3. I can structure a multi-service codebase cleanly.
4. I can make the project easy to run and evaluate without hiding behind private credentials.
5. I can improve an initial MVP into a more infrastructure-grade system over time.

Because this repo is public, I made the platform mock-first. Reviewers can run the whole stack locally without third-party API keys, but the code also supports optional real OpenAI calls through environment variables.

## Architecture

The project is split into a small monorepo:

- `apps/api`: Rust API service for workflow authoring, publishing, triggering, recovery actions, and observability endpoints.
- `apps/worker`: Rust worker that polls the queue, executes steps, handles retries, dead-letter transitions, and advances schedules.
- `apps/dashboard`: React dashboard for workflows, JSON editing, filtering runs, inspecting dead letters, replaying failures, and triggering runs.
- `crates/engine`: Shared execution engine, data models, migrations, seed data, API handlers, and worker logic.

High-level flow:

```text
Client / Dashboard
        |
        v
   Workflow API
        |
        v
   Postgres-backed run state
        |
        v
      Worker
        |
   +----+----+----+
   |         |    |
 HTTP      AI    DB
 step     step  write
```

## Current Version

The repo is now at v1.3 from a product milestone point of view.

- v1.0 established the MVP: workflow definitions, sequential execution, retries, idempotency, observability, and a local zero-secrets demo path.
- v1.2 added operational failure tooling: dead-letter records, replay lineage, retry-now controls, run filtering, and a dedicated dead-letter panel.
- v1.3 adds workflow-language branching: `if` steps, a structured condition DSL, persisted branch decisions, and a read-only workflow map in the dashboard.

## Core Features

### 1. Versioned Workflow Definitions

I implemented `draft` and `published` workflow states. Runs always bind to a published version, which means editing a draft does not mutate in-flight or future published executions until I explicitly publish again.

### 2. Sequential Job Execution

The worker executes one step at a time, persists the run context after each success, and stores every step attempt in Postgres. That makes the system resumable after worker crashes.

I still keep execution globally sequential in v1.3, but I now persist an active execution plan in run context. When the worker hits an `if` step, it evaluates the condition, records the chosen path, expands the selected branch into the active plan, and continues without turning the runtime into a full DAG scheduler.

### 3. Retry and Backoff

Each workflow has a retry policy with:

- max attempts
- initial retry delay
- backoff multiplier
- max interval
- jitter

This gives the engine realistic infrastructure behavior instead of a naive single-attempt flow.

### 4. Idempotency

I added idempotency at two levels:

- workflow trigger idempotency, so duplicate requests can return the same run
- step-level outbound idempotency propagation for HTTP execution

That matters because retries without deduplication are how systems accidentally create duplicate payments, emails, or writes.

### 5. Dead-Letter and Recovery Flow

When a run exhausts retries, I now preserve that terminal failure as immutable history and create a dead-letter record with:

- failed step index
- failed step name
- terminal error
- last attempt count
- original run id

From there, I can:

- inspect the failure in the dashboard
- force a waiting retry immediately
- replay a failed run as a brand-new run without mutating the original

That separation matters for auditability.

### 6. Observability Dashboard

The dashboard shows:

- workflows
- draft JSON definitions
- conditional branch examples and JSON parse feedback
- a read-only workflow map for linear and branched flows
- latest runs
- status and trigger type
- retry state
- step-by-step attempt history
- dead-letter records
- branch decisions for each run
- input and accumulated context

I intentionally kept the editor JSON-based rather than visual, because the target user here is a developer and the goal is to demonstrate engine design over no-code UX.

### 7. Conditional Branching

In v1.3 I added a new workflow step type:

- `type: "if"`

That control step supports a structured condition DSL with:

- `equals`
- `not_equals`
- `contains`
- `exists`
- `gt`
- `lt`

Conditions resolve against:

- `input`
- previous step outputs such as `steps.0.output.customer_id`

The branch choice is persisted in run context so retries, dead-lettering, and replay stay deterministic for the same input and workflow version.

### 8. Secret-Free Demo Mode

I seeded demo workflows and a demo API key so this repo works immediately in local development. If `OPENAI_API_KEY` is not present, AI steps use a deterministic mock summarizer.

## Example Demo Workflows

I included three workflows to make the project easier to demo:

- `user-signup`
  - mock billing customer creation
  - mock welcome email
  - branch on plan tier
  - AI summary generation for `pro` users
  - durable artifact write for either branch
- `document-summarize`
  - mock OCR extraction
  - AI summarization
  - artifact persistence
- `scrape-and-brief`
  - mock scrape step
  - intentional first-attempt failure
  - retry and recovery
  - dead-letter transition if attempts are exhausted
  - replayable failure path

The `user-signup` flow is now the best branching demo because it shows a clean `if/else` path in both the JSON editor and the workflow map. The `scrape-and-brief` flow is still useful because it visibly demonstrates retries, dead-lettering, and replay.

## Tech Stack

- Rust
- Axum
- SQLx
- PostgreSQL
- React
- Vite
- Docker Compose

I chose Rust for the backend because I wanted this project to signal systems depth and careful backend engineering.

## Local Setup

### Prerequisites

- Docker Desktop
- Rust toolchain
- Node.js 22+
- npm

### Run the Full Stack

```bash
docker compose up --build
```

Services:

- API: `http://localhost:8000`
- Dashboard: `http://localhost:5173`
- Postgres: `localhost:5432`

Demo credentials:

- API key: `demo_api_key`
- Workspace: `RelayFlow Demo`

## Demo Script

If I only had 3 to 5 minutes in an interview, this is the demo flow I would use:

1. Start the stack with `docker compose up --build`.
2. Open the dashboard and use the `Quick Start` section.
3. Click `Easiest demo` to load `user-signup`.
4. Click `Run selected workflow`.
5. Open the latest run and explain:
   - the published workflow version
   - the sequential execution model
   - the stored run context
   - the recorded branch decision for the `if` step
6. Switch to `scrape-and-brief`.
7. Click `Run selected workflow` again.
8. Open the run and explain:
   - the failed first attempt
   - retry scheduling
   - dead-letter creation on terminal failure
   - replay as a new run instead of mutating history

If I were showing only one workflow, I would pick:

- `user-signup` for branching and normal execution
- `scrape-and-brief` for retries, failure, and recovery

### Run Services Individually

API:

```bash
cargo run -p workflow-api
```

Worker:

```bash
cargo run -p workflow-worker
```

Dashboard:

```bash
cd apps/dashboard
npm install
npm run dev
```

## API Surface

Main workflow endpoints:

- `POST /v1/workflows`
- `PUT /v1/workflows/:id/draft`
- `POST /v1/workflows/:id/publish`
- `POST /v1/workflows/:slug/run`
- `POST /v1/webhooks/:token`
- `GET /v1/workflows/:id/history`

Observability and ops endpoints:

- `GET /v1/runs`
- `GET /v1/runs/:id`
- `GET /v1/dead-letters`
- `POST /v1/runs/:id/retry-now`
- `POST /v1/runs/:id/replay`
- `GET /v1/usage`

The run listing now supports additive filtering through query params:

- `status`
- `workflow_id`
- `trigger_kind`
- `dead_lettered`

For authenticated API calls, I use:

```http
x-api-key: demo_api_key
```

## What I Would Talk Through In An Interview

If I were walking an interviewer through this repo, I would focus on:

- why I modeled workflow versions separately from workflow drafts
- how persisted step attempts make retries inspectable and crash-safe
- why I stored branch decisions in run context instead of introducing a DAG scheduler too early
- how the persisted execution plan keeps branching deterministic under retry and replay
- why the engine is explicitly at-least-once rather than pretending to be exactly-once
- where idempotency is enforced and why that matters for external side effects
- why dead-letter records are separate from replay runs
- why replay creates a new run instead of mutating the original failure
- why I used a mock-first architecture for a public portfolio project
- how I would evolve this from sequential workflows into DAG execution later

## Two-Minute Talk Track

If I needed to summarize the project quickly, I would say:

> I built Relay-Flow as a developer-first execution engine for AI and API workflows. The core idea is that calling external APIs is easy, but making those workflows reliable is hard. So instead of building a no-code tool, I focused on the execution layer: published workflow versions, sequential durable execution, retries with backoff, idempotency, dead-letter recovery, and run history.
>
> The backend is written in Rust with Postgres as the source of truth. The worker persists every attempt and updates run context after each step, so the system can recover cleanly and remain inspectable. In v1.3 I added conditional branching, but I intentionally implemented it as a persisted execution plan instead of jumping straight to a DAG scheduler. That let me keep the runtime simple while still proving I can design workflow language features in a way that stays deterministic under retry and replay.
>
> I also kept the repo mock-first so anyone can run it locally with no secrets, which matters for a public interview project. The result is a system that is small enough to explain clearly, but still shows real infrastructure thinking rather than CRUD or toy automation.

## Tradeoffs and Current Limitations

I intentionally kept the system narrow:

- sequential execution only
- conditional branching only, not full DAG, fan-out, or fan-in yet
- cron parsing is intentionally lightweight for demo scope
- auth is single-workspace and simplified
- billing is represented through seeded plans and usage limits, not live Stripe checkout
- connectors are generic and mock-first instead of a full marketplace
- dead-letter handling is designed for local operational clarity, not yet for distributed multi-tenant production scale
- workflow visualization is read-only; the JSON editor remains the source of truth

Those tradeoffs were deliberate. I wanted a smaller system that demonstrates reliability well, rather than a larger system with shallow internals.

## What I Would Build Next

If I continued this project, my next steps would be:

- full DAG execution model
- reusable condition groups and richer branch predicates
- stronger rate-limit policies per workflow and connector
- richer tracing and step-level metrics
- dead-letter replay policies and bulk recovery actions
- webhook signature verification
- multi-workspace auth and roles
- deployable hosted control plane

## Verification

I verified the codebase with:

```bash
cargo check
cargo test
cd apps/dashboard && npm run build
```

## Repo Goal

I built this project to be something I can open in VS Code during an interview and explain end-to-end:

- the product idea
- the architecture
- the failure model
- the persistence model
- the recovery model
- the local developer experience
- the tradeoffs I made

That was the goal from the start, and every part of the implementation is shaped around that.
