import { useEffect, useState } from 'react';

const API_BASE = import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:8000';
const API_KEY = import.meta.env.VITE_API_KEY ?? 'demo_api_key';

type Workflow = {
  id: string;
  slug: string;
  name: string;
  description?: string;
  webhook_token: string;
  has_published_version: boolean;
  draft_definition: unknown;
  published_definition?: unknown;
  updated_at: string;
};

type Run = {
  id: string;
  workflow_id: string;
  workflow_name: string;
  workflow_slug: string;
  status: string;
  trigger_kind: string;
  error?: string;
  created_at: string;
  next_retry_at?: string;
  dead_lettered: boolean;
  replayed_from_run_id?: string;
  replay_run_id?: string;
};

type DeadLetter = {
  id: string;
  run_id: string;
  workflow_id: string;
  workflow_name: string;
  workflow_slug: string;
  failed_step_index: number;
  failed_step_name: string;
  terminal_error: string;
  last_attempt: number;
  created_at: string;
  replay_run_id?: string;
};

type RunDetail = {
  run: Run;
  input: unknown;
  context: unknown;
  dead_letter?: DeadLetter;
  attempts: Array<{
    id: string;
    step_index: number;
    step_name: string;
    status: string;
    attempt: number;
    error?: string;
    started_at: string;
    finished_at?: string;
    next_retry_at?: string;
    output?: unknown;
  }>;
};

type Usage = {
  workspace_name: string;
  plan: string;
  monthly_limit: number;
  executions_this_month: number;
  remaining: number;
};

type RunActionResponse = {
  run_id: string;
  status: string;
  related_run_id?: string;
  deduplicated: boolean;
};

type TriggerConfig = {
  api?: boolean;
  webhook?: boolean;
  cron?: string | null;
};

type RetryPolicyDraft = {
  max_attempts?: number;
  initial_interval_seconds?: number;
  backoff_multiplier?: number;
  max_interval_seconds?: number;
  jitter_ratio?: number;
};

type WorkflowCondition = {
  path: string;
  operator: string;
  value?: unknown;
};

type WorkflowStepNode = {
  type: string;
  name: string;
  condition?: WorkflowCondition;
  then_steps?: WorkflowStepNode[];
  else_steps?: WorkflowStepNode[];
};

type WorkflowDefinitionDraft = {
  name?: string;
  description?: string;
  concurrency_limit?: number | null;
  triggers?: TriggerConfig;
  retry_policy?: RetryPolicyDraft;
  steps?: WorkflowStepNode[];
};

type BranchDecision = {
  step_name: string;
  step_index: number;
  matched: boolean;
  chosen_branch: string;
  inserted_steps: string[];
  evaluated_at: string;
  condition: WorkflowCondition;
};

type RunContextPayload = {
  input?: unknown;
  steps?: Array<{ name: string; output: unknown; finished_at: string }>;
  execution_plan?: WorkflowStepNode[];
  branch_decisions?: BranchDecision[];
};

type GuideStep = {
  id: string;
  label: string;
  title: string;
  summary: string;
  bullets: string[];
  code?: string;
  note?: string;
};

const prettyJson = (value: unknown) => JSON.stringify(value, null, 2);

const IF_STEP_EXAMPLE = prettyJson({
  type: 'if',
  name: 'branch-on-plan',
  condition: {
    path: 'input.plan',
    operator: 'equals',
    value: 'pro',
  },
});

function parseDraftDefinition(
  raw: string,
): { definition: WorkflowDefinitionDraft | null; error: string | null } {
  try {
    return { definition: JSON.parse(raw) as WorkflowDefinitionDraft, error: null };
  } catch (error) {
    return {
      definition: null,
      error: error instanceof Error ? error.message : 'Draft JSON is invalid.',
    };
  }
}

function readRunContext(value: unknown): RunContextPayload | null {
  if (!value || typeof value !== 'object') {
    return null;
  }
  return value as RunContextPayload;
}

function describeCondition(condition?: WorkflowCondition) {
  if (!condition) {
    return 'Branch condition';
  }
  const renderedValue =
    condition.value === undefined ? '' : ` ${JSON.stringify(condition.value)}`;
  return `${condition.path} ${condition.operator}${renderedValue}`;
}

function renderWorkflowNodes(
  steps: WorkflowStepNode[],
  scope = 'root',
): JSX.Element[] {
  return steps.map((step, index) => {
    const key = `${scope}-${index}-${step.name}`;
    if (step.type === 'if') {
      return (
        <article key={key} className="workflow-node branch-node">
          <div className="workflow-node-header">
            <strong>{step.name}</strong>
            <small>if</small>
          </div>
          <p>{describeCondition(step.condition)}</p>
          <div className="branch-columns">
            <div className="branch-column">
              <span className="branch-heading">Then</span>
              <div className="branch-stack">
                {step.then_steps && step.then_steps.length > 0 ? (
                  renderWorkflowNodes(step.then_steps, `${key}-then`)
                ) : (
                  <div className="empty-branch">No steps</div>
                )}
              </div>
            </div>
            <div className="branch-column">
              <span className="branch-heading">Else</span>
              <div className="branch-stack">
                {step.else_steps && step.else_steps.length > 0 ? (
                  renderWorkflowNodes(step.else_steps, `${key}-else`)
                ) : (
                  <div className="empty-branch">Skipped when false</div>
                )}
              </div>
            </div>
          </div>
        </article>
      );
    }

    return (
      <article key={key} className="workflow-node">
        <div className="workflow-node-header">
          <strong>{step.name}</strong>
          <small>{step.type}</small>
        </div>
      </article>
    );
  });
}

function samplePayloadForWorkflow(slug?: string | null) {
  switch (slug) {
    case 'user-signup':
      return prettyJson({
        user_id: 'candidate_001',
        email: 'candidate@example.com',
        plan: 'pro',
      });
    case 'document-summarize':
      return prettyJson({
        document_id: 'doc_001',
        source_text:
          'RelayFlow gives developers a reliable way to orchestrate APIs and AI steps with retries and observability.',
      });
    case 'scrape-and-brief':
      return prettyJson({ url: 'https://relayflow.dev/blog/reliability' });
    default:
      return prettyJson({
        user_id: 'candidate_001',
        email: 'candidate@example.com',
        plan: 'pro',
      });
  }
}

function buildTriggerCurl(slug?: string | null) {
  const workflowSlug = slug ?? 'user-signup';
  return [
    `curl -X POST ${API_BASE}/v1/workflows/${workflowSlug}/run \\`,
    `  -H "Content-Type: application/json" \\`,
    `  -H "x-api-key: ${API_KEY}" \\`,
    `  -d '{"payload": ${samplePayloadForWorkflow(workflowSlug)}}'`,
  ].join('\n');
}

function buildWebhookCurl(workflow?: Workflow | null) {
  if (!workflow) {
    return 'Select a workflow to generate a webhook example.';
  }
  return [
    `curl -X POST ${API_BASE}/v1/webhooks/${workflow.webhook_token} \\`,
    `  -H "Content-Type: application/json" \\`,
    `  -d '${samplePayloadForWorkflow(workflow.slug)}'`,
  ].join('\n');
}

function triggerModes(definition?: WorkflowDefinitionDraft | null) {
  if (!definition?.triggers) {
    return 'api';
  }

  const modes: string[] = [];
  if (definition.triggers.api !== false) modes.push('api');
  if (definition.triggers.webhook) modes.push('webhook');
  if (definition.triggers.cron) modes.push(`cron ${definition.triggers.cron}`);
  return modes.join(' | ');
}

function buildGuideSteps(
  workflow?: Workflow | null,
  definition?: WorkflowDefinitionDraft | null,
): GuideStep[] {
  return [
    {
      id: 'stack',
      label: '1. Start',
      title: 'Start the local stack',
      summary:
        'RelayFlow is meant to be runnable in a public repo with no secret setup. The fastest path is Docker Compose.',
      bullets: [
        'Start Docker Desktop, then run docker compose up --build from the repo root.',
        'Open the dashboard at http://localhost:5173 and the API at http://localhost:8000.',
        `Use the seeded API key ${API_KEY} for authenticated requests.`,
      ],
      code: 'docker compose up --build',
      note:
        'If Docker is unavailable, run the Rust API, Rust worker, and Vite dashboard separately.',
    },
    {
      id: 'author',
      label: '2. Author',
      title: 'Author or inspect a workflow',
      summary:
        'The JSON editor is the source of truth. Drafts and published versions are separate so runs always bind to an immutable version.',
      bullets: [
        `The selected workflow is ${workflow?.slug ?? 'user-signup'}${definition?.steps ? ` with ${definition.steps.length} top-level steps.` : '.'}`,
        `Triggers for the selected workflow: ${triggerModes(definition)}.`,
        'Use type "if" to branch on input or prior step output without turning the runtime into a full DAG engine.',
      ],
      code: IF_STEP_EXAMPLE,
      note:
        'Save draft updates the editable version. Publish creates the immutable version used for future runs.',
    },
    {
      id: 'trigger',
      label: '3. Trigger',
      title: 'Trigger a run from the UI or API',
      summary:
        'The dashboard can trigger runs directly, but the public API surface is still central to the project story.',
      bullets: [
        'Load a sample payload for the selected workflow so the demo path is immediately runnable.',
        'The dashboard generates a unique idempotency key for each manual run.',
        'You can also copy a curl example and trigger the same flow outside the UI.',
      ],
      code: buildTriggerCurl(workflow?.slug),
      note: workflow
        ? `Webhook example available for ${workflow.slug} using token ${workflow.webhook_token}.`
        : undefined,
    },
    {
      id: 'observe',
      label: '4. Observe',
      title: 'Inspect retries, branches, and context',
      summary:
        'Once a run starts, the worker persists attempt history and accumulated context after every step.',
      bullets: [
        'The runs panel lets me filter by status, workflow, trigger kind, and dead-letter state.',
        'The execution timeline shows attempts, outputs, dead-letter metadata, and branch decisions.',
        'The workflow map is read-only on purpose. It exists to explain the JSON definition, not replace it.',
      ],
      note:
        'For branched runs, the worker stores the chosen branch in run context so retries and replay do not re-evaluate into a different path.',
    },
    {
      id: 'recover',
      label: '5. Recover',
      title: 'Handle failure and recovery',
      summary:
        'The project is strongest when it demonstrates what happens after an API call fails.',
      bullets: [
        'Retrying runs can be forced immediately with Retry now.',
        'Terminal failures create dead-letter records exactly once for audit clarity.',
        'Replay creates a brand-new run linked back to the failed run instead of mutating history.',
      ],
      note:
        'The scrape-and-brief workflow intentionally fails its first mock scrape attempt, which makes it the best failure demo.',
    },
  ];
}

async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`${API_BASE}${path}`, {
    ...init,
    headers: {
      'Content-Type': 'application/json',
      'x-api-key': API_KEY,
      ...(init?.headers ?? {}),
    },
  });
  if (!response.ok) {
    const error = await response
      .json()
      .catch(() => ({ error: response.statusText }));
    throw new Error(error.error || response.statusText);
  }
  return response.json() as Promise<T>;
}

function buildRunQuery(filters: {
  status: string;
  workflowId: string;
  triggerKind: string;
  deadLetteredOnly: boolean;
}) {
  const params = new URLSearchParams();
  if (filters.status) params.set('status', filters.status);
  if (filters.workflowId) params.set('workflow_id', filters.workflowId);
  if (filters.triggerKind) params.set('trigger_kind', filters.triggerKind);
  if (filters.deadLetteredOnly) params.set('dead_lettered', 'true');
  const query = params.toString();
  return query ? `/v1/runs?${query}` : '/v1/runs';
}

export default function App() {
  const [workflows, setWorkflows] = useState<Workflow[]>([]);
  const [runs, setRuns] = useState<Run[]>([]);
  const [deadLetters, setDeadLetters] = useState<DeadLetter[]>([]);
  const [usage, setUsage] = useState<Usage | null>(null);
  const [selectedWorkflowId, setSelectedWorkflowId] = useState<string | null>(null);
  const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
  const [draftJson, setDraftJson] = useState('');
  const [runPayload, setRunPayload] = useState(samplePayloadForWorkflow('user-signup'));
  const [runDetail, setRunDetail] = useState<RunDetail | null>(null);
  const [message, setMessage] = useState<string>(
    'Demo mode is active. No third-party keys are required.',
  );
  const [loading, setLoading] = useState(false);
  const [statusFilter, setStatusFilter] = useState('');
  const [workflowFilter, setWorkflowFilter] = useState('');
  const [triggerFilter, setTriggerFilter] = useState('');
  const [deadLetterOnly, setDeadLetterOnly] = useState(false);
  const [activeGuideId, setActiveGuideId] = useState('stack');

  const draftPreview = parseDraftDefinition(draftJson);
  const runContext = readRunContext(runDetail?.context);
  const branchDecisions = runContext?.branch_decisions ?? [];
  const selectedWorkflow =
    workflows.find((workflow) => workflow.id === selectedWorkflowId) ?? null;
  const guideSteps = buildGuideSteps(selectedWorkflow, draftPreview.definition);
  const activeGuide =
    guideSteps.find((step) => step.id === activeGuideId) ?? guideSteps[0];

  async function refresh() {
    const runPath = buildRunQuery({
      status: statusFilter,
      workflowId: workflowFilter,
      triggerKind: triggerFilter,
      deadLetteredOnly: deadLetterOnly,
    });

    const [workflowData, runData, usageData, deadLetterData] = await Promise.all([
      api<Workflow[]>('/v1/workflows'),
      api<Run[]>(runPath),
      api<Usage>('/v1/usage'),
      api<DeadLetter[]>('/v1/dead-letters'),
    ]);
    setWorkflows(workflowData);
    setRuns(runData);
    setUsage(usageData);
    setDeadLetters(deadLetterData);
    if (!selectedWorkflowId && workflowData.length > 0) {
      setSelectedWorkflowId(workflowData[0].id);
      setDraftJson(prettyJson(workflowData[0].draft_definition));
      setRunPayload(samplePayloadForWorkflow(workflowData[0].slug));
    }
  }

  useEffect(() => {
    refresh().catch((error) => setMessage(error.message));
    const interval = window.setInterval(() => {
      refresh().catch(() => undefined);
      if (selectedRunId) {
        api<RunDetail>(`/v1/runs/${selectedRunId}`)
          .then(setRunDetail)
          .catch(() => undefined);
      }
    }, 3000);
    return () => window.clearInterval(interval);
  }, [selectedRunId, statusFilter, workflowFilter, triggerFilter, deadLetterOnly]);

  useEffect(() => {
    if (selectedWorkflow) {
      setDraftJson(prettyJson(selectedWorkflow.draft_definition));
      setRunPayload(samplePayloadForWorkflow(selectedWorkflow.slug));
    }
  }, [selectedWorkflow]);

  async function copyText(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text);
      setMessage(`${label} copied to clipboard.`);
    } catch {
      setMessage(`Failed to copy ${label.toLowerCase()}.`);
    }
  }

  async function saveDraft() {
    if (!selectedWorkflow) return;
    setLoading(true);
    try {
      const definition = JSON.parse(draftJson);
      await api(`/v1/workflows/${selectedWorkflow.id}/draft`, {
        method: 'PUT',
        body: JSON.stringify({ definition }),
      });
      setMessage(`Draft saved for ${selectedWorkflow.slug}`);
      await refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to save draft');
    } finally {
      setLoading(false);
    }
  }

  async function publishWorkflow() {
    if (!selectedWorkflow) return;
    setLoading(true);
    try {
      await api(`/v1/workflows/${selectedWorkflow.id}/publish`, {
        method: 'POST',
      });
      setMessage(`Published ${selectedWorkflow.slug}`);
      await refresh();
    } catch (error) {
      setMessage(
        error instanceof Error ? error.message : 'Failed to publish workflow',
      );
    } finally {
      setLoading(false);
    }
  }

  async function triggerSelectedWorkflow() {
    if (!selectedWorkflow) return;
    setLoading(true);
    try {
      const payload = JSON.parse(runPayload);
      const result = await api<{ run_id: string; deduplicated: boolean }>(
        `/v1/workflows/${selectedWorkflow.slug}/run`,
        {
          method: 'POST',
          body: JSON.stringify({
            payload,
            idempotency_key: `dashboard-${selectedWorkflow.slug}-${crypto.randomUUID()}`,
          }),
        },
      );
      setSelectedRunId(result.run_id);
      setMessage(
        result.deduplicated
          ? 'Existing run returned from idempotency key.'
          : 'Workflow triggered.',
      );
      await refresh();
      const detail = await api<RunDetail>(`/v1/runs/${result.run_id}`);
      setRunDetail(detail);
      setActiveGuideId('observe');
    } catch (error) {
      setMessage(
        error instanceof Error ? error.message : 'Failed to trigger workflow',
      );
    } finally {
      setLoading(false);
    }
  }

  async function loadRun(runId: string) {
    setSelectedRunId(runId);
    try {
      const detail = await api<RunDetail>(`/v1/runs/${runId}`);
      setRunDetail(detail);
      setActiveGuideId('observe');
    } catch (error) {
      setMessage(
        error instanceof Error ? error.message : 'Failed to load run detail',
      );
    }
  }

  async function retryNow() {
    if (!runDetail) return;
    setLoading(true);
    try {
      await api<RunActionResponse>(`/v1/runs/${runDetail.run.id}/retry-now`, {
        method: 'POST',
      });
      setMessage('Run moved to immediate retry.');
      await refresh();
      const detail = await api<RunDetail>(`/v1/runs/${runDetail.run.id}`);
      setRunDetail(detail);
      setActiveGuideId('recover');
    } catch (error) {
      setMessage(
        error instanceof Error ? error.message : 'Failed to retry run now',
      );
    } finally {
      setLoading(false);
    }
  }

  async function replayRun() {
    if (!runDetail) return;
    setLoading(true);
    try {
      const result = await api<RunActionResponse>(
        `/v1/runs/${runDetail.run.id}/replay`,
        { method: 'POST' },
      );
      setSelectedRunId(result.run_id);
      setMessage(
        result.deduplicated
          ? 'Existing replay run returned.'
          : 'Replay run queued.',
      );
      await refresh();
      const detail = await api<RunDetail>(`/v1/runs/${result.run_id}`);
      setRunDetail(detail);
      setActiveGuideId('recover');
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to replay run');
    } finally {
      setLoading(false);
    }
  }

  return (
    <div className="app-shell">
      <header className="hero">
        <div>
          <p className="eyebrow">Reliable AI & API Workflows</p>
          <h1>RelayFlow</h1>
          <p className="hero-copy">
            A developer-first execution engine with retries, idempotency, job
            history, dead-letter handling, conditional branching, and a
            zero-secrets demo path.
          </p>
          <div className="hero-badges">
            <span>At-least-once execution</span>
            <span>Branch-aware retries</span>
            <span>Zero-secrets demo</span>
          </div>
        </div>
        <div className="hero-card">
          <span>Workspace</span>
          <strong>{usage?.workspace_name ?? 'Loading...'}</strong>
          <span>Plan</span>
          <strong>{usage?.plan ?? 'demo'}</strong>
          <span>Executions</span>
          <strong>
            {usage?.executions_this_month ?? 0}/{usage?.monthly_limit ?? 0}
          </strong>
        </div>
      </header>

      <section className="panel tutorial-panel">
        <div className="panel-header">
          <h2>Guided Tutorial</h2>
          <span>Start here if you are evaluating the project for the first time</span>
        </div>
        <div className="tutorial-grid">
          <div className="tutorial-nav">
            {guideSteps.map((step) => (
              <button
                key={step.id}
                className={
                  step.id === activeGuide.id
                    ? 'tutorial-step-button active'
                    : 'tutorial-step-button'
                }
                onClick={() => setActiveGuideId(step.id)}
              >
                <small>{step.label}</small>
                <strong>{step.title}</strong>
              </button>
            ))}
          </div>
          <div className="tutorial-content">
            <div className="tutorial-copy">
              <span className="tutorial-label">{activeGuide.label}</span>
              <h3>{activeGuide.title}</h3>
              <p>{activeGuide.summary}</p>
              <ul className="tutorial-list">
                {activeGuide.bullets.map((bullet) => (
                  <li key={bullet}>{bullet}</li>
                ))}
              </ul>
              {activeGuide.note ? (
                <div className="tutorial-note">{activeGuide.note}</div>
              ) : null}
            </div>
            {activeGuide.code ? (
              <pre className="tutorial-code">{activeGuide.code}</pre>
            ) : null}
          </div>
        </div>
      </section>

      <main className="layout">
        <section className="panel workflows">
          <div className="panel-header">
            <h2>Workflows</h2>
            <span>{workflows.length} loaded</span>
          </div>
          <div className="workflow-list">
            {workflows.map((workflow) => (
              <button
                key={workflow.id}
                className={
                  workflow.id === selectedWorkflowId
                    ? 'workflow-item active'
                    : 'workflow-item'
                }
                onClick={() => setSelectedWorkflowId(workflow.id)}
              >
                <strong>{workflow.name}</strong>
                <span>{workflow.slug}</span>
                <small>
                  {workflow.has_published_version ? 'Published' : 'Draft only'}
                </small>
              </button>
            ))}
          </div>
        </section>

        <section className="panel editor">
          <div className="panel-header">
            <h2>JSON Editor</h2>
            <span>{selectedWorkflow?.slug ?? 'Select a workflow'}</span>
          </div>
          <textarea
            value={draftJson}
            onChange={(event) => setDraftJson(event.target.value)}
            spellCheck={false}
          />
          <div className="actions">
            <button onClick={saveDraft} disabled={!selectedWorkflow || loading}>
              Save draft
            </button>
            <button
              onClick={publishWorkflow}
              disabled={!selectedWorkflow || loading}
            >
              Publish
            </button>
            <button
              onClick={triggerSelectedWorkflow}
              disabled={!selectedWorkflow || loading}
            >
              Trigger run
            </button>
          </div>

          <div className="subpanel workflow-guide">
            <div className="panel-header">
              <h3>Selected workflow guide</h3>
              <span>{selectedWorkflow?.slug ?? 'No workflow selected'}</span>
            </div>
            <div className="detail-grid">
              <div className="detail-card">
                <span>Triggers</span>
                <strong>{triggerModes(draftPreview.definition)}</strong>
              </div>
              <div className="detail-card">
                <span>Top-level steps</span>
                <strong>{draftPreview.definition?.steps?.length ?? 0}</strong>
              </div>
              <div className="detail-card">
                <span>Retry attempts</span>
                <strong>{draftPreview.definition?.retry_policy?.max_attempts ?? 3}</strong>
              </div>
              <div className="detail-card">
                <span>Concurrency</span>
                <strong>{draftPreview.definition?.concurrency_limit ?? 2}</strong>
              </div>
            </div>
            <div className="secondary-actions">
              <button
                onClick={() =>
                  setRunPayload(samplePayloadForWorkflow(selectedWorkflow?.slug))
                }
                disabled={!selectedWorkflow}
              >
                Load sample payload
              </button>
              <button
                onClick={() =>
                  copyText(
                    buildTriggerCurl(selectedWorkflow?.slug),
                    'Trigger curl',
                  )
                }
                disabled={!selectedWorkflow}
              >
                Copy trigger curl
              </button>
              <button
                onClick={() =>
                  copyText(buildWebhookCurl(selectedWorkflow), 'Webhook curl')
                }
                disabled={!selectedWorkflow}
              >
                Copy webhook curl
              </button>
            </div>
          </div>

          <div className="subpanel">
            <h3>Test payload</h3>
            <textarea
              value={runPayload}
              onChange={(event) => setRunPayload(event.target.value)}
              spellCheck={false}
            />
          </div>
          <div className="subpanel branch-help">
            <h3>Conditional branch example</h3>
            <p>
              Use <code>type: "if"</code> with a structured condition. Paths
              resolve against <code>input</code> and executed step outputs like{' '}
              <code>steps.0.output.customer_id</code>.
            </p>
            <pre>{IF_STEP_EXAMPLE}</pre>
            <div
              className={
                draftPreview.error
                  ? 'validation-state invalid'
                  : 'validation-state valid'
              }
            >
              {draftPreview.error
                ? `Draft JSON error: ${draftPreview.error}`
                : 'Draft JSON parsed successfully. Save or publish to run backend validation for if-step structure.'}
            </div>
          </div>
          <div className="subpanel workflow-map-panel">
            <div className="panel-header">
              <h3>Workflow map</h3>
              <span>Read-only preview</span>
            </div>
            {draftPreview.definition?.steps &&
            draftPreview.definition.steps.length > 0 ? (
              <div className="workflow-map">
                {renderWorkflowNodes(draftPreview.definition.steps)}
              </div>
            ) : (
              <div className="empty-state">
                Parse a workflow definition to preview linear steps and
                conditional branches.
              </div>
            )}
          </div>
        </section>

        <section className="panel runs">
          <div className="panel-header">
            <h2>Runs</h2>
            <span>Failure ops</span>
          </div>

          <div className="filter-grid">
            <label>
              <span>Status</span>
              <select
                value={statusFilter}
                onChange={(event) => setStatusFilter(event.target.value)}
              >
                <option value="">All</option>
                <option value="queued">Queued</option>
                <option value="running">Running</option>
                <option value="retrying">Retrying</option>
                <option value="failed">Failed</option>
                <option value="succeeded">Succeeded</option>
              </select>
            </label>
            <label>
              <span>Workflow</span>
              <select
                value={workflowFilter}
                onChange={(event) => setWorkflowFilter(event.target.value)}
              >
                <option value="">All</option>
                {workflows.map((workflow) => (
                  <option key={workflow.id} value={workflow.id}>
                    {workflow.slug}
                  </option>
                ))}
              </select>
            </label>
            <label>
              <span>Trigger</span>
              <select
                value={triggerFilter}
                onChange={(event) => setTriggerFilter(event.target.value)}
              >
                <option value="">All</option>
                <option value="api">API</option>
                <option value="webhook">Webhook</option>
                <option value="cron">Cron</option>
                <option value="replay">Replay</option>
              </select>
            </label>
            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={deadLetterOnly}
                onChange={(event) => setDeadLetterOnly(event.target.checked)}
              />
              <span>Dead-lettered only</span>
            </label>
          </div>

          <div className="run-list">
            {runs.map((run) => (
              <button
                key={run.id}
                className={run.id === selectedRunId ? 'run-item active' : 'run-item'}
                onClick={() => loadRun(run.id)}
              >
                <div>
                  <strong>{run.workflow_name}</strong>
                  <span>
                    {run.dead_lettered
                      ? `${run.trigger_kind} | dead-lettered`
                      : run.trigger_kind}
                  </span>
                </div>
                <div>
                  <small className={`status ${run.status}`}>{run.status}</small>
                  <small>{new Date(run.created_at).toLocaleString()}</small>
                </div>
              </button>
            ))}
          </div>

          <div className="subpanel dead-letter-panel">
            <div className="panel-header">
              <h3>Dead letters</h3>
              <span>{deadLetters.length} tracked</span>
            </div>
            <div className="dead-letter-list">
              {deadLetters.map((deadLetter) => (
                <button
                  key={deadLetter.id}
                  className="dead-letter-item"
                  onClick={() => loadRun(deadLetter.run_id)}
                >
                  <strong>{deadLetter.workflow_slug}</strong>
                  <span>
                    Step {deadLetter.failed_step_index + 1}:{' '}
                    {deadLetter.failed_step_name}
                  </span>
                  <small>
                    {deadLetter.replay_run_id
                      ? 'Replay created'
                      : 'Needs operator action'}
                  </small>
                </button>
              ))}
            </div>
          </div>
        </section>

        <section className="panel timeline">
          <div className="panel-header">
            <h2>Execution timeline</h2>
            <span>{runDetail?.run.workflow_name ?? 'Choose a run'}</span>
          </div>
          {runDetail ? (
            <div className="timeline-content">
              <div className="timeline-summary">
                <div>
                  <span>Status</span>
                  <strong className={`status ${runDetail.run.status}`}>
                    {runDetail.run.status}
                  </strong>
                </div>
                <div>
                  <span>Trigger</span>
                  <strong>{runDetail.run.trigger_kind}</strong>
                </div>
                <div>
                  <span>Next retry</span>
                  <strong>
                    {runDetail.run.next_retry_at
                      ? new Date(runDetail.run.next_retry_at).toLocaleString()
                      : 'n/a'}
                  </strong>
                </div>
              </div>

              <div className="operation-row">
                <button
                  onClick={retryNow}
                  disabled={loading || runDetail.run.status !== 'retrying'}
                >
                  Retry now
                </button>
                <button
                  onClick={replayRun}
                  disabled={
                    loading ||
                    !(runDetail.run.dead_lettered || runDetail.run.status === 'failed')
                  }
                >
                  Replay run
                </button>
              </div>

              {runDetail.dead_letter ? (
                <div className="dead-letter-callout">
                  <strong>Dead-letter record</strong>
                  <span>
                    Step {runDetail.dead_letter.failed_step_index + 1}:{' '}
                    {runDetail.dead_letter.failed_step_name}
                  </span>
                  <p>{runDetail.dead_letter.terminal_error}</p>
                </div>
              ) : null}

              {branchDecisions.length > 0 ? (
                <div className="branch-decision-grid">
                  {branchDecisions.map((decision) => (
                    <article
                      key={`${decision.step_name}-${decision.step_index}`}
                      className="branch-decision-card"
                    >
                      <header>
                        <strong>{decision.step_name}</strong>
                        <small>{decision.chosen_branch}</small>
                      </header>
                      <p>{describeCondition(decision.condition)}</p>
                      <span>
                        {decision.matched
                          ? 'Condition matched'
                          : 'Condition did not match'}
                      </span>
                      <small>
                        {decision.inserted_steps.length > 0
                          ? `Expanded to: ${decision.inserted_steps.join(' -> ')}`
                          : 'No steps inserted for this branch.'}
                      </small>
                    </article>
                  ))}
                </div>
              ) : null}

              <div className="timeline-attempts">
                {runDetail.attempts.map((attempt) => (
                  <article key={attempt.id} className="attempt-card">
                    <header>
                      <div>
                        <strong>
                          {attempt.step_index + 1}. {attempt.step_name}
                        </strong>
                        <span>Attempt {attempt.attempt}</span>
                      </div>
                      <small className={`status ${attempt.status}`}>
                        {attempt.status}
                      </small>
                    </header>
                    <p>{attempt.error ?? 'Completed without error.'}</p>
                    <pre>{prettyJson(attempt.output ?? {})}</pre>
                  </article>
                ))}
              </div>
              <div className="context-grid">
                <div>
                  <h3>Input</h3>
                  <pre>{prettyJson(runDetail.input)}</pre>
                </div>
                <div>
                  <h3>Context</h3>
                  <pre>{prettyJson(runDetail.context)}</pre>
                </div>
              </div>
            </div>
          ) : (
            <div className="empty-state">
              Select a run to inspect retries, dead letters, outputs, replay
              actions, and branch decisions.
            </div>
          )}
        </section>
      </main>

      <footer className="status-bar">
        <span>{message}</span>
        <span>API key for demo: {API_KEY}</span>
      </footer>
    </div>
  );
}
