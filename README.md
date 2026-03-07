# Relay-Flow

I built Relay-Flow as an interview project to show that I can design and ship backend infrastructure, not just CRUD apps. The product is a developer-first execution engine for AI and API workflows with reliability guarantees: retries, backoff, idempotency, observability, and job history.

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

The engine then executes those steps sequentially and persists state between each transition. If a step fails, the system records the attempt, calculates the next retry time, and resumes later without losing the run history.

## Why I Built It This Way

I wanted this project to show four things clearly in interviews:

1. I can build backend systems with real operational concerns.
2. I understand workflow state, retries, idempotency, and failure recovery.
3. I can structure a multi-service codebase cleanly.
4. I can make the project easy to run and evaluate without hiding behind private credentials.

Because this repo is public, I made the platform mock-first. Reviewers can run the whole stack locally without third-party API keys, but the code also supports optional real OpenAI calls through environment variables.

## Architecture

The project is split into a small monorepo:

- `apps/api`: Rust API service for workflow authoring, publishing, triggering, and observability endpoints.
- `apps/worker`: Rust worker that polls the queue, executes steps, handles retries, and advances schedules.
- `apps/dashboard`: React dashboard for workflows, JSON editing, triggering runs, and inspecting execution timelines.
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

## Core Features

### 1. Versioned Workflow Definitions

I implemented `draft` and `published` workflow states. Runs always bind to a published version, which means editing a draft does not mutate in-flight or future published executions until I explicitly publish again.

### 2. Sequential Job Execution

The worker executes one step at a time, persists the run context after each success, and stores every step attempt in Postgres. That makes the system resumable after worker crashes.

### 3. Retry and Backoff

Each workflow has a retry policy with:

- max attempts
- initial retry delay
- backoff multiplier
- max interval
- jitter

This gives the engine realistic infrastructure behavior instead of a naive “try once” flow.

### 4. Idempotency

I added idempotency at two levels:

- workflow trigger idempotency, so duplicate requests can return the same run
- step-level outbound idempotency propagation for HTTP execution

That matters because “retry” without deduplication is how systems accidentally create duplicate payments, emails, or writes.

### 5. Observability Dashboard

The dashboard shows:

- workflows
- draft JSON definitions
- latest runs
- execution status
- retry state
- step-by-step attempt history
- input and accumulated context

I intentionally kept the editor JSON-based rather than visual, because the target user here is a developer and the goal is to demonstrate engine design over no-code UX.

### 6. Secret-Free Demo Mode

I seeded demo workflows and a demo API key so this repo works immediately in local development. If `OPENAI_API_KEY` is not present, AI steps use a deterministic mock summarizer.

## Example Demo Workflows

I included three workflows to make the project easier to demo:

- `user-signup`
  - mock billing customer creation
  - mock welcome email
  - AI summary generation
  - durable artifact write
- `document-summarize`
  - mock OCR extraction
  - AI summarization
  - artifact persistence
- `scrape-and-brief`
  - mock scrape step
  - intentional first-attempt failure
  - retry and recovery
  - summary persistence

The `scrape-and-brief` flow is especially useful in interviews because it visibly demonstrates the retry system working.

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

Main endpoints:

- `POST /v1/workflows`
- `PUT /v1/workflows/:id/draft`
- `POST /v1/workflows/:id/publish`
- `POST /v1/workflows/:slug/run`
- `POST /v1/webhooks/:token`
- `GET /v1/runs`
- `GET /v1/runs/:id`
- `GET /v1/workflows/:id/history`
- `GET /v1/usage`

For authenticated API calls, I use:

```http
x-api-key: demo_api_key
```

## What I’d Talk Through In An Interview

If I were walking an interviewer through this repo, I’d focus on:

- why I modeled workflow versions separately from workflow drafts
- how persisted step attempts make retries inspectable and crash-safe
- why the engine is explicitly at-least-once rather than pretending to be exactly-once
- where idempotency is enforced and why that matters for external side effects
- why I used a mock-first architecture for a public portfolio project
- how I would evolve this from sequential workflows into DAG execution later

## Tradeoffs and Current Limitations

I intentionally kept the MVP narrow:

- sequential execution only
- no DAG/fan-out/fan-in yet
- cron parsing is intentionally lightweight for demo scope
- auth is single-workspace and simplified
- billing is represented through seeded plans and usage limits, not live Stripe checkout
- connectors are generic/mock-first instead of a full marketplace

Those tradeoffs were deliberate. I wanted a smaller system that demonstrates reliability well, rather than a larger system with shallow internals.

## What I’d Build Next

If I continued this project, my next steps would be:

- full DAG execution model
- better rate-limit policies per workflow and per connector
- dead-letter queue handling
- richer webhook verification
- step-level metrics and tracing
- multi-workspace auth and roles
- deployable hosted control plane

## Verification

I verified the codebase with:

```bash
cargo check
cd apps/dashboard && npm run build
```

## Repo Goal

I built this project to be something I can open in VS Code during an interview and explain end-to-end:

- the product idea
- the architecture
- the failure model
- the persistence model
- the local developer experience
- the tradeoffs I made

That was the goal from the start, and every part of the implementation is shaped around that.
