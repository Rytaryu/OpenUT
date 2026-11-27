import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('./owner-reply.sh', import.meta.url));
// Pin the real script for this test process, even if a local editor updates it.
const scriptSource = readFileSync(script, 'utf8');
const repository = 'example/openut';
const base = `repos/${repository}`;
const source = 27;
const wakePr = 40;

// Every child invocation uses this executable instead of the real gh. Its
// JSON file survives separate bash runs so failures and retries are causal.
const fakeGh = String.raw`
import { readFileSync, writeFileSync } from 'node:fs';
const statePath = process.env.OWNER_REPLY_FAKE_STATE;
const state = JSON.parse(readFileSync(statePath, 'utf8'));
const args = process.argv.slice(2);
if (args.shift() !== 'api') throw new Error('Expected gh api');
let method;
let path;
let query;
let include = false;
let silent = false;
const fields = {};
for (let i = 0; i < args.length; i++) {
  if (args[i] === '-X') method = args[++i];
  else if (args[i] === '--jq') query = args[++i];
  else if (args[i] === '--include') include = true;
  else if (args[i] === '--silent') silent = true;
  else if (args[i] === '-f') {
    const field = args[++i];
    const equal = field.indexOf('=');
    fields[field.slice(0, equal)] = field.slice(equal + 1);
  } else if (!path) path = args[i];
  else throw new Error('Unexpected gh argument: ' + args[i]);
}
method ??= Object.keys(fields).length ? 'POST' : 'GET';
const call = { method, path, query, fields, include, silent };
state.calls.push(call);
const save = () => writeFileSync(statePath, JSON.stringify(state));
const emit = (status, output) => {
  if (include && status) {
    const reason = status === 200 ? 'OK' : status === 404 ? 'Not Found' : status === 403 ? 'Forbidden' : 'Bad Gateway';
    process.stdout.write('HTTP/2.0 ' + status + ' ' + reason + '\r\nContent-Type: application/json\r\n\r\n');
  }
  if (!silent) process.stdout.write(output + '\n');
};
const fail = (status) => {
  call.failed = status;
  save();
  if (status) emit(status, JSON.stringify({ message: 'fake HTTP error' }));
  process.stderr.write(status ? 'HTTP ' + status + ': fake gh response\n' : 'Network unavailable\n');
  process.exit(1);
};
const injected = state.failures.find((entry) =>
  entry.remaining && entry.method === method && entry.path === path && (!entry.query || entry.query === query));
if (injected) {
  injected.remaining--;
  fail(injected.status ?? 502);
}
if (path.endsWith('/contents/docs/project/PAUSED')) {
  if (!state.paused) fail(404);
  save();
  emit(200, '{}');
  process.exit(0);
}
const matched = path.match(/^repos\/example\/openut\/issues\/(\d+)(?:\/(.*))?$/);
if (!matched) throw new Error('Unexpected fake endpoint: ' + path);
const number = matched[1];
const suffix = matched[2];
const issue = state.issues[number];
if (!issue) fail(404);
let output;
if (method === 'GET' && !suffix) {
  if (query === '.state') output = issue.state;
  else if (query === '.labels[].name | select(startswith("role:") or . == "backlog")')
    output = issue.labels.filter((label) => label.startsWith('role:') || label === 'backlog').join('\n');
  else if (!query) output = JSON.stringify({ ...issue, labels: issue.labels.map((name) => ({ name })) });
  else throw new Error('Unexpected fake jq query: ' + query);
} else if (method === 'DELETE' && suffix?.startsWith('labels/')) {
  const label = decodeURIComponent(suffix.slice('labels/'.length));
  const index = issue.labels.indexOf(label);
  if (index < 0) fail(404);
  issue.labels.splice(index, 1);
  output = '{}';
} else if (method === 'POST' && suffix === 'labels') {
  const label = fields['labels[]'];
  if (!label) throw new Error('Missing label');
  if (!issue.labels.includes(label)) issue.labels.push(label);
  output = '{}';
} else if (method === 'POST' && suffix === 'comments') {
  if (!fields.body) throw new Error('Missing comment body');
  issue.comments.push(fields.body);
  output = '{}';
} else throw new Error('Unexpected fake API operation: ' + JSON.stringify(call));
save();
emit(200, output);
`;

function issue(body = '', labels = [], state = 'open') {
  return { body, labels: [...labels], state, comments: [] };
}

function harness(
  t,
  {
    body,
    labels = ['needs-human', 'question'],
    paused = false,
    failures = [],
    targetState = 'open',
    omitTarget = false,
    locale = 'C.UTF-8',
  } = {},
) {
  const directory = mkdtempSync(join(tmpdir(), 'owner-reply-test-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const statePath = join(directory, 'state.json');
  const ghPath = join(directory, 'gh');
  const scriptPath = join(directory, 'owner-reply.sh');
  writeFileSync(scriptPath, scriptSource, { mode: 0o755 });
  writeFileSync(ghPath, `#!${process.execPath}\n${fakeGh}`, { mode: 0o755 });
  const issues = {
    [source]: issue(body, labels),
    [wakePr]: issue('Wake comments only', ['wake']),
    14: issue('UX work', ['backlog']),
    41: issue('Unrelated review', ['role:review', 'enhancement']),
  };
  if (!omitTarget)
    issues[55] = issue(
      'Unrelated implementation',
      ['enhancement', 'role:impl', 'backlog'],
      targetState,
    );
  writeFileSync(
    statePath,
    JSON.stringify({ paused, issues, calls: [], failures }),
  );
  const read = () => JSON.parse(readFileSync(statePath, 'utf8'));
  return {
    read,
    run() {
      return spawnSync('bash', [scriptPath], {
        encoding: 'utf8',
        timeout: 10_000,
        env: {
          ...process.env,
          PATH: `${directory}:${process.env.PATH ?? ''}`,
          REPO: repository,
          ISSUE: String(source),
          WAKE_PR: String(wakePr),
          COMMENT_URL: 'https://example.invalid/owner-comment',
          GH_TOKEN: 'fake-token-unused',
          GITHUB_TOKEN: '',
          LC_ALL: locale,
          OWNER_REPLY_FAKE_STATE: statePath,
        },
      });
    },
  };
}

function succeeded(result) {
  assert.equal(result.signal, null, result.stderr);
  assert.equal(result.status, 0, result.stderr);
}

function mutations(state) {
  return state.calls.filter(({ method }) => method !== 'GET');
}

function labels(state, number) {
  return [...state.issues[number].labels].sort();
}

function assertSourceFallback(state) {
  assert.deepEqual(labels(state, source), ['question', 'role:pm']);
  assert.equal(state.issues[source].comments.length, 1);
  assert.match(state.issues[source].comments[0], /`role:pm` の #27/);
  assert.deepEqual(state.issues[wakePr].comments, []);
}

function assertCompletionFlagLast(state) {
  const writes = mutations(state);
  assert.equal(writes.at(-1).method, 'DELETE');
  assert.equal(
    writes.at(-1).path,
    `${base}/issues/${source}/labels/needs-human`,
  );
  assert.equal(writes.at(-1).failed, undefined);
}

for (const [role, target] of [
  ['role:foo', 55],
  ['role:reveiw', 41],
]) {
  test(`unknown role ${role} falls back on the source without changing #${target}`, (t) => {
    const fixture = harness(t, { body: `返事の行き先: ${role} #${target}` });
    const before = fixture.read().issues[target];
    succeeded(fixture.run());
    const state = fixture.read();
    assertSourceFallback(state);
    assert.deepEqual(state.issues[target], before);
    assert.equal(
      mutations(state).some(({ path }) =>
        path.startsWith(`${base}/issues/${target}/`),
      ),
      false,
    );
    assertCompletionFlagLast(state);
  });
}

for (const body of ['返事の行き先: #55', 'No destination line', null]) {
  test(`missing role keeps an external parsed destination untouched (${String(body)})`, (t) => {
    const fixture = harness(t, { body });
    const before = fixture.read().issues[55];
    succeeded(fixture.run());
    const state = fixture.read();
    assertSourceFallback(state);
    assert.deepEqual(state.issues[55], before);
    assertCompletionFlagLast(state);
  });
}

test('routes an allowed role to a different target and wakes it before clearing needs-human', (t) => {
  const fixture = harness(t, {
    body: '返事の行き先: role:review #55',
    labels: ['question', 'needs-human'],
  });
  succeeded(fixture.run());
  const state = fixture.read();
  assert.deepEqual(labels(state, 55), ['enhancement', 'role:review']);
  assert.deepEqual(labels(state, source), ['question']);
  assert.equal(state.issues[55].comments.length, 1);
  assert.match(state.issues[55].comments[0], /オーナーが #27 に返事しました/);
  assert.deepEqual(state.issues[wakePr].comments, [
    'WAKE role:review #55',
  ]);
  assertCompletionFlagLast(state);
});

test('routes an allowed role to the source and removes its old role and backlog labels', (t) => {
  const fixture = harness(t, {
    body: '返事の行き先: role:ux',
    labels: ['needs-human', 'question', 'backlog', 'role:pm'],
  });
  succeeded(fixture.run());
  const state = fixture.read();
  assert.deepEqual(labels(state, source), ['question', 'role:ux']);
  assert.equal(state.issues[source].comments.length, 1);
  assert.deepEqual(state.issues[wakePr].comments, ['WAKE role:ux #27']);
  assertCompletionFlagLast(state);
});

for (const locale of ['C', 'C.UTF-8']) {
  test(`accepts a full-width colon and CRLF in ${locale}`, (t) => {
    const fixture = harness(t, {
      body: '説明\r\n返事の行き先：role:ux #14\r\n',
      locale,
    });
    succeeded(fixture.run());
    const state = fixture.read();
    assert.deepEqual(labels(state, 14), ['role:ux']);
    assert.deepEqual(state.issues[wakePr].comments, ['WAKE role:ux #14']);
    assertCompletionFlagLast(state);
  });
}

for (const options of [{ labels: ['question'] }, { paused: true }]) {
  test(`does not mutate state when ${options.paused ? 'PAUSED exists' : 'needs-human is absent'}`, (t) => {
    const fixture = harness(t, {
      body: '返事の行き先: role:review #55',
      ...options,
    });
    const before = fixture.read().issues;
    succeeded(fixture.run());
    const state = fixture.read();
    assert.deepEqual(state.issues, before);
    assert.deepEqual(mutations(state), []);
  });
}

for (const options of [{ targetState: 'closed' }, { omitTarget: true }]) {
  test(`falls back on the source when the target is ${options.omitTarget ? 'missing' : 'closed'}`, (t) => {
    const fixture = harness(t, {
      body: '返事の行き先: role:review #55',
      ...options,
    });
    const before = fixture.read().issues[55];
    succeeded(fixture.run());
    const state = fixture.read();
    assertSourceFallback(state);
    assert.deepEqual(state.issues[55], before);
    assertCompletionFlagLast(state);
  });
}

test('routes an explicitly valid PM destination without an event wake', (t) => {
  const fixture = harness(t, { body: '返事の行き先: role:pm #55' });
  succeeded(fixture.run());
  const state = fixture.read();
  assert.deepEqual(labels(state, 55), ['enhancement', 'role:pm']);
  assert.deepEqual(labels(state, source), ['question']);
  assert.equal(state.issues[55].comments.length, 1);
  assert.deepEqual(state.issues[wakePr].comments, []);
  assertCompletionFlagLast(state);
});

for (const [name, path, status, query] of [
  ['PAUSED lookup 502', `${base}/contents/docs/project/PAUSED`, 502],
  [
    'PAUSED lookup authorization failure',
    `${base}/contents/docs/project/PAUSED`,
    403,
  ],
  ['PAUSED lookup network failure', `${base}/contents/docs/project/PAUSED`, 0],
  ['target lookup 502', `${base}/issues/55`, 502],
  [
    'target label-list lookup 502',
    `${base}/issues/55`,
    502,
    '.labels[].name | select(startswith("role:") or . == "backlog")',
  ],
]) {
  test(`aborts without mutations on ${name}, retaining a retryable source`, (t) => {
    const fixture = harness(t, {
      body: '返事の行き先: role:review #55',
      failures: [{ method: 'GET', path, remaining: 1, status, query }],
    });
    const before = fixture.read().issues;
    const failed = fixture.run();
    assert.notEqual(failed.status, 0);
    const first = fixture.read();
    assert.deepEqual(first.issues, before);
    assert.deepEqual(mutations(first), []);
    assert.equal(first.issues[source].labels.includes('needs-human'), true);
    assert.equal(first.failures[0].remaining, 0);
    succeeded(fixture.run());
    const state = fixture.read();
    assert.deepEqual(labels(state, 55), ['enhancement', 'role:review']);
    assert.deepEqual(labels(state, source), ['question']);
    assert.deepEqual(state.issues[wakePr].comments, [
      'WAKE role:review #55',
    ]);
    assertCompletionFlagLast(state);
  });
}

for (const [name, method, path] of [
  ['target label addition', 'POST', `${base}/issues/55/labels`],
  ['target comment', 'POST', `${base}/issues/55/comments`],
  ['source comment', 'POST', `${base}/issues/${source}/comments`],
  ['wake comment', 'POST', `${base}/issues/${wakePr}/comments`],
  [
    'final completion flag removal',
    'DELETE',
    `${base}/issues/${source}/labels/needs-human`,
  ],
]) {
  test(`retains needs-human after one 502 at ${name}, then succeeds on retry`, (t) => {
    const fixture = harness(t, {
      body: '返事の行き先: role:review #55',
      failures: [{ method, path, remaining: 1 }],
    });
    const failed = fixture.run();
    assert.notEqual(failed.status, 0);
    assert.match(failed.stderr, /HTTP 502/);
    const first = fixture.read();
    assert.equal(first.issues[source].labels.includes('needs-human'), true);
    assert.equal(first.failures[0].remaining, 0);
    assert.equal(
      first.calls.some(
        (call) =>
          call.method === 'DELETE' &&
          call.path.endsWith('/labels/needs-human') &&
          call.failed === undefined,
      ),
      false,
    );
    succeeded(fixture.run());
    const state = fixture.read();
    assert.deepEqual(labels(state, 55), ['enhancement', 'role:review']);
    assert.deepEqual(labels(state, source), ['question']);
    assert.ok(state.issues[wakePr].comments.length >= 1);
    for (const comment of state.issues[wakePr].comments)
      assert.equal(comment, 'WAKE role:review #55');
    assert.ok(state.issues[source].comments.length >= 1);
    assert.ok(state.issues[55].comments.length >= 1);
    assertCompletionFlagLast(state);
  });
}
