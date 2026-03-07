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

const prettyJson = (value: unknown) => JSON.stringify(value, null, 2);

const IF_STEP_EXAMPLE = prettyJson({
  type: 'if',
  name: 'branch-on-plan',
  condition: {
    path: 'input.plan',
    operator: 'equals',
    value: 'pro',
  },
  then_steps: [
    {
      type: 'ai.openai',
      name: 'generate-summary',
      prompt: 'Summarize onboarding for {{input.email}}.',
    },
    {
      type: 'db.postgres',
      name: 'store-pro-artifact',
      record: {
        plan: '{{input.plan}}',
        summary: '{{steps.2.output.summary}}',
      },
    },
  ],
  else_steps: [
    {
      type: 'db.postgres',
      name: 'store-standard-artifact',
      record: {
        summary: 'AI summary skipped for non-pro signup',
      },
    },
  ],
});

function parseDraftDefinition(raw: string): { definition: WorkflowDefinitionDraft | null; error: string | null } {
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
  const renderedValue = condition.value === undefined ? '' : ` ${JSON.stringify(condition.value)}`;
  return `${condition.path} ${condition.operator}${renderedValue}`;
}

function renderWorkflowNodes(steps: WorkflowStepNode[], scope = 'root'): JSX.Element[] {
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
                {step.then_steps && step.then_steps.length > 0 ? renderWorkflowNodes(step.then_steps, `${key}-then`) : <div className="empty-branch">No steps</div>}
              </div>
            </div>
            <div className="branch-column">
              <span className="branch-heading">Else</span>
              <div className="branch-stack">
                {step.else_steps && step.else_steps.length > 0 ? renderWorkflowNodes(step.else_steps, `${key}-else`) : <div className="empty-branch">Skipped when false</div>}
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
    const error = await response.json().catch(() => ({ error: response.statusText }));
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
  const [runPayload, setRunPayload] = useState('{\n  "user_id": "candidate_001",\n  "email": "candidate@example.com",\n  "plan": "pro"\n}');
  const [runDetail, setRunDetail] = useState<RunDetail | null>(null);
  const [message, setMessage] = useState<string>('Demo mode is active. No third-party keys are required.');
  const [loading, setLoading] = useState(false);
  const [statusFilter, setStatusFilter] = useState('');
  const [workflowFilter, setWorkflowFilter] = useState('');
  const [triggerFilter, setTriggerFilter] = useState('');
  const [deadLetterOnly, setDeadLetterOnly] = useState(false);

  const draftPreview = parseDraftDefinition(draftJson);
  const runContext = readRunContext(runDetail?.context);
  const branchDecisions = runContext?.branch_decisions ?? [];

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
    }
  }

  useEffect(() => {
    refresh().catch((error) => setMessage(error.message));
    const interval = window.setInterval(() => {
      refresh().catch(() => undefined);
      if (selectedRunId) {
        api<RunDetail>(`/v1/runs/${selectedRunId}`).then(setRunDetail).catch(() => undefined);
      }
    }, 3000);
    return () => window.clearInterval(interval);
  }, [selectedRunId, statusFilter, workflowFilter, triggerFilter, deadLetterOnly]);

  const selectedWorkflow = workflows.find((workflow) => workflow.id === selectedWorkflowId) ?? null;

  useEffect(() => {
    if (selectedWorkflow) {
      setDraftJson(prettyJson(selectedWorkflow.draft_definition));
    }
  }, [selectedWorkflow, selectedWorkflowId, workflows]);

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
      await api(`/v1/workflows/${selectedWorkflow.id}/publish`, { method: 'POST' });
      setMessage(`Published ${selectedWorkflow.slug}`);
      await refresh();
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to publish workflow');
    } finally {
      setLoading(false);
    }
  }

  async function triggerSelectedWorkflow() {
    if (!selectedWorkflow) return;
    setLoading(true);
    try {
      const payload = JSON.parse(runPayload);
      const result = await api<{ run_id: string; deduplicated: boolean }>(`/v1/workflows/${selectedWorkflow.slug}/run`, {
        method: 'POST',
        body: JSON.stringify({
          payload,
          idempotency_key: `dashboard-${selectedWorkflow.slug}-${crypto.randomUUID()}`,
        }),
      });
      setSelectedRunId(result.run_id);
      setMessage(result.deduplicated ? 'Existing run returned from idempotency key.' : 'Workflow triggered.');
      await refresh();
      const detail = await api<RunDetail>(`/v1/runs/${result.run_id}`);
      setRunDetail(detail);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to trigger workflow');
    } finally {
      setLoading(false);
    }
  }

  async function loadRun(runId: string) {
    setSelectedRunId(runId);
    try {
      const detail = await api<RunDetail>(`/v1/runs/${runId}`);
      setRunDetail(detail);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to load run detail');
    }
  }

  async function retryNow() {
    if (!runDetail) return;
    setLoading(true);
    try {
      await api<RunActionResponse>(`/v1/runs/${runDetail.run.id}/retry-now`, { method: 'POST' });
      setMessage('Run moved to immediate retry.');
      await refresh();
      const detail = await api<RunDetail>(`/v1/runs/${runDetail.run.id}`);
      setRunDetail(detail);
    } catch (error) {
      setMessage(error instanceof Error ? error.message : 'Failed to retry run now');
    } finally {
      setLoading(false);
    }
  }

  async function replayRun() {
    if (!runDetail) return;
    setLoading(true);
    try {
      const result = await api<RunActionResponse>(`/v1/runs/${runDetail.run.id}/replay`, { method: 'POST' });
      setSelectedRunId(result.run_id);
      setMessage(result.deduplicated ? 'Existing replay run returned.' : 'Replay run queued.');
      await refresh();
      const detail = await api<RunDetail>(`/v1/runs/${result.run_id}`);
      setRunDetail(detail);
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
            A developer-first execution engine with retries, idempotency, job history, dead-letter handling, and a zero-secrets demo path.
          </p>
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
                className={workflow.id === selectedWorkflowId ? 'workflow-item active' : 'workflow-item'}
                onClick={() => setSelectedWorkflowId(workflow.id)}
              >
                <strong>{workflow.name}</strong>
                <span>{workflow.slug}</span>
                <small>{workflow.has_published_version ? 'Published' : 'Draft only'}</small>
              </button>
            ))}
          </div>
        </section>

        <section className="panel editor">
          <div className="panel-header">
            <h2>JSON Editor</h2>
            <span>{selectedWorkflow?.slug ?? 'Select a workflow'}</span>
          </div>
          <textarea value={draftJson} onChange={(event) => setDraftJson(event.target.value)} spellCheck={false} />
          <div className="actions">
            <button onClick={saveDraft} disabled={!selectedWorkflow || loading}>
              Save draft
            </button>
            <button onClick={publishWorkflow} disabled={!selectedWorkflow || loading}>
              Publish
            </button>
            <button onClick={triggerSelectedWorkflow} disabled={!selectedWorkflow || loading}>
              Trigger run
            </button>
          </div>
          <div className="subpanel">
            <h3>Test payload</h3>
            <textarea value={runPayload} onChange={(event) => setRunPayload(event.target.value)} spellCheck={false} />
          </div>
          <div className="subpanel branch-help">
            <h3>Conditional branch example</h3>
            <p>
              Use <code>type: "if"</code> with a structured condition. Paths resolve against <code>input</code> and executed step outputs like{' '}
              <code>steps.0.output.customer_id</code>.
            </p>
            <pre>{IF_STEP_EXAMPLE}</pre>
            <div className={draftPreview.error ? 'validation-state invalid' : 'validation-state valid'}>
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
            {draftPreview.definition?.steps && draftPreview.definition.steps.length > 0 ? (
              <div className="workflow-map">{renderWorkflowNodes(draftPreview.definition.steps)}</div>
            ) : (
              <div className="empty-state">Parse a workflow definition to preview linear steps and conditional branches.</div>
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
              <select value={statusFilter} onChange={(event) => setStatusFilter(event.target.value)}>
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
              <select value={workflowFilter} onChange={(event) => setWorkflowFilter(event.target.value)}>
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
              <select value={triggerFilter} onChange={(event) => setTriggerFilter(event.target.value)}>
                <option value="">All</option>
                <option value="api">API</option>
                <option value="webhook">Webhook</option>
                <option value="cron">Cron</option>
                <option value="replay">Replay</option>
              </select>
            </label>
            <label className="checkbox-row">
              <input type="checkbox" checked={deadLetterOnly} onChange={(event) => setDeadLetterOnly(event.target.checked)} />
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
                  <span>{run.dead_lettered ? `${run.trigger_kind} | dead-lettered` : run.trigger_kind}</span>
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
                <button key={deadLetter.id} className="dead-letter-item" onClick={() => loadRun(deadLetter.run_id)}>
                  <strong>{deadLetter.workflow_slug}</strong>
                  <span>
                    Step {deadLetter.failed_step_index + 1}: {deadLetter.failed_step_name}
                  </span>
                  <small>{deadLetter.replay_run_id ? 'Replay created' : 'Needs operator action'}</small>
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
                  <strong className={`status ${runDetail.run.status}`}>{runDetail.run.status}</strong>
                </div>
                <div>
                  <span>Trigger</span>
                  <strong>{runDetail.run.trigger_kind}</strong>
                </div>
                <div>
                  <span>Next retry</span>
                  <strong>{runDetail.run.next_retry_at ? new Date(runDetail.run.next_retry_at).toLocaleString() : 'n/a'}</strong>
                </div>
              </div>

              <div className="operation-row">
                <button onClick={retryNow} disabled={loading || runDetail.run.status !== 'retrying'}>
                  Retry now
                </button>
                <button onClick={replayRun} disabled={loading || !(runDetail.run.dead_lettered || runDetail.run.status === 'failed')}>
                  Replay run
                </button>
              </div>

              {runDetail.dead_letter ? (
                <div className="dead-letter-callout">
                  <strong>Dead-letter record</strong>
                  <span>
                    Step {runDetail.dead_letter.failed_step_index + 1}: {runDetail.dead_letter.failed_step_name}
                  </span>
                  <p>{runDetail.dead_letter.terminal_error}</p>
                </div>
              ) : null}

              {branchDecisions.length > 0 ? (
                <div className="branch-decision-grid">
                  {branchDecisions.map((decision) => (
                    <article key={`${decision.step_name}-${decision.step_index}`} className="branch-decision-card">
                      <header>
                        <strong>{decision.step_name}</strong>
                        <small>{decision.chosen_branch}</small>
                      </header>
                      <p>{describeCondition(decision.condition)}</p>
                      <span>{decision.matched ? 'Condition matched' : 'Condition did not match'}</span>
                      <small>
                        {decision.inserted_steps.length > 0 ? `Expanded to: ${decision.inserted_steps.join(' -> ')}` : 'No steps inserted for this branch.'}
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
                      <small className={`status ${attempt.status}`}>{attempt.status}</small>
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
            <div className="empty-state">Select a run to inspect retries, dead letters, outputs, replay actions, and branch decisions.</div>
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
