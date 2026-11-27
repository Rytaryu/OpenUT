import assert from 'node:assert/strict';
import test from 'node:test';
import {
  authorRole,
  ciState,
  closingIssues,
  isApprovalOf,
  isHeld,
  isImplementationPr,
  latestReview,
  mergeMethod,
  parseBacklog,
  requiredReviewerApp,
  run,
  touchedPaths,
} from './handoff.mjs';

const OWNER = 'owner';
const REPO = `${OWNER}/openut`;
const WAKE_PR = 1;
const SHA = 'abcdef1234567890';
const viaClaude = { slug: 'claude' };
const viaChatgpt = { slug: 'chatgpt-codex-connector' };
const comment = (body, app = null, login = OWNER) => ({ body, user: { login }, performed_via_github_app: app });

test('reviewer, merge method and author role follow the changed paths', () => {
  assert.equal(requiredReviewerApp(['docs/design/overview.md']), 'chatgpt-codex-connector');
  assert.equal(requiredReviewerApp(['docs/design/ui/a.html', 'src/openut/a.py']), 'claude');
  assert.equal(requiredReviewerApp([]), 'claude');
  assert.equal(mergeMethod(['docs/design/a.md', 'docs/test/b.md']), 'squash');
  assert.equal(mergeMethod(['docs/a.md', 'src/openut/a.py']), 'merge');
  assert.equal(authorRole(['docs/design/ui/x.css']), 'role:ux');
  assert.equal(authorRole(['docs/design/ui/x.css', 'docs/design/a.md']), 'role:design');
  assert.equal(authorRole(['src/openut/a.py']), 'role:impl');
  assert.equal(authorRole(['src/openut/a.py', 'tests/test_a.py']), 'role:impl');
  assert.equal(authorRole(['pyproject.toml']), 'role:impl');
  assert.equal(authorRole([]), 'role:impl');
  // Tests and test plans alone belong to the tester.
  assert.equal(authorRole(['tests/test_replay.py', 'docs/test/plan.md']), 'role:test');
  // The process documents, the research notes and the hand-off belong to the PM.
  assert.equal(authorRole(['README.md', 'AGENTS.md', 'docs/research/findings.md']), 'role:pm');
  assert.equal(authorRole(['.github/workflows/ci.yml', '.github/scripts/handoff.mjs']), 'role:pm');
  assert.equal(authorRole(['README.md', 'src/openut/a.py']), 'role:impl');
  assert.equal(authorRole(['docs/design/a.md', 'docs/project/STATUS.md']), 'role:pm');
  // A file moved out of src/ into docs/design/ is still an implementation change.
  const moved = touchedPaths([{ filename: 'docs/design/a.md', previous_filename: 'src/openut/a.py' }]);
  assert.equal(requiredReviewerApp(moved), 'claude');
  assert.equal(authorRole(moved), 'role:impl');
  // Only PRs of the implementer count toward the cap on open implementation PRs.
  assert.equal(isImplementationPr(['docs/project/STATUS.md', 'docs/test/plan.md']), false);
  assert.equal(isImplementationPr(['.github/workflows/ci.yml']), false);
  assert.equal(isImplementationPr(['src/openut/a.py', 'docs/a.md']), true);
});

test('only the owning reviewer (or the owner) can approve, and only the head commit', () => {
  const src = ['src/openut/a.py'];
  const approve = `REVIEW: APPROVE\n\n対象: \`${SHA.slice(0, 7)}\``;
  // The implementer (ChatGPT) cannot approve an implementation PR.
  assert.equal(latestReview([comment(approve, viaChatgpt)], src, OWNER), null);
  assert.ok(isApprovalOf(latestReview([comment(approve, viaClaude)], src, OWNER), SHA));
  assert.ok(isApprovalOf(latestReview([comment(approve)], src, OWNER), SHA));
  // Someone else's account never counts (the repository is public).
  assert.equal(latestReview([comment(approve, null, 'stranger')], src, OWNER), null);
  assert.equal(latestReview([comment(approve, viaClaude, 'stranger')], src, OWNER), null);
  // A later REQUEST_CHANGES wins; an approval of an older commit does not count.
  const later = [comment(approve, viaClaude), comment('REVIEW: REQUEST_CHANGES\n対象: abcdef1', viaClaude)];
  assert.equal(isApprovalOf(latestReview(later, src, OWNER), SHA), false);
  assert.equal(isApprovalOf(comment('REVIEW: APPROVE\n対象: `1111111`', viaClaude), SHA), false);
  // Only the first SHA on the 対象 line counts; SHAs elsewhere do not.
  assert.equal(isApprovalOf(comment(`REVIEW: APPROVE\n対象: 1111111\n前回（${SHA.slice(0, 7)}）の [必須] は直った`, viaClaude), SHA), false);
  assert.equal(isApprovalOf(comment(`REVIEW: APPROVE\n\n${SHA.slice(0, 7)} を見た`, viaClaude), SHA), false);
  assert.ok(isApprovalOf(comment(`REVIEW: APPROVE\r\n対象： ${SHA}`, viaClaude), SHA));
  // "REVIEW:" must start the comment; a quoted mention is not a review.
  assert.equal(latestReview([comment(`上の REVIEW: APPROVE は ${SHA}`, viaClaude)], src, OWNER), null);
  // Design PRs are approved by the design reviewer (ChatGPT), not by Claude.
  assert.equal(latestReview([comment(approve, viaClaude)], ['docs/design/a.md'], OWNER), null);
  assert.ok(latestReview([comment(approve, viaChatgpt)], ['docs/design/a.md'], OWNER));
});

test('a マージの条件 line holds the merge until it is confirmed', () => {
  assert.equal(isHeld('説明', []), false);
  assert.equal(isHeld('マージの条件: オーナーの了承', []), true);
  assert.equal(isHeld('', [comment('前置き\nマージの条件： オーナーの了承')]), true);
  assert.equal(isHeld('', [comment('マージの条件: A'), comment('確認しました。\nマージの条件を確認: OK')]), false);
  assert.equal(isHeld('', [comment('マージの条件を確認: OK'), comment('マージの条件: B')]), true);
  assert.equal(isHeld('', [comment('- `マージの条件:` の行はない'), comment('| `マージの条件:` | OK |')]), false);
});

test('the newest CI run on a commit decides', () => {
  const r = (created_at, status, conclusion = null) => ({ created_at, status, conclusion });
  assert.equal(ciState([]).state, 'pending');
  assert.equal(ciState([r('2026-11-28T01:00:00Z', 'completed', 'success')]).state, 'success');
  assert.equal(ciState([r('2026-11-28T01:00:00Z', 'completed', 'failure')]).state, 'failure');
  assert.equal(ciState([r('2026-11-28T01:00:00Z', 'completed', 'timed_out')]).state, 'failure');
  // A run that was cancelled or is still going is nothing to act on.
  assert.equal(ciState([r('2026-11-28T01:00:00Z', 'completed', 'cancelled')]).state, 'pending');
  assert.equal(ciState([r('2026-11-28T01:00:00Z', 'in_progress')]).state, 'pending');
  // A re-run after a failure wins, in either order of the list.
  const failed = r('2026-11-28T01:00:00Z', 'completed', 'failure');
  const passed = r('2026-11-28T02:00:00Z', 'completed', 'success');
  assert.equal(ciState([failed, passed]).state, 'success');
  assert.equal(ciState([passed, failed]).state, 'success');
  assert.equal(ciState([passed, r('2026-11-28T03:00:00Z', 'queued')]).state, 'pending');
});

test('backlog headers', () => {
  assert.deepEqual(parseBacklog('担当: role:impl\n着手の条件: なし（#43 は閉じた）\n\n本文'), { role: 'role:impl', conditions: [] });
  assert.deepEqual(parseBacklog('担当: role:test\r\n着手の条件: #53 が閉じている\r\n'), {
    role: 'role:test',
    conditions: [{ number: 53, kind: 'closed' }],
  });
  assert.deepEqual(parseBacklog('担当: role:impl\n着手の条件: #44 が閉じている、#14 がマージ済み').conditions, [
    { number: 44, kind: 'closed' },
    { number: 14, kind: 'merged' },
  ]);
  assert.equal(parseBacklog('担当: role:impl\n着手の条件: オーナーが了承したら'), null);
  assert.equal(parseBacklog('担当: role:pm\n着手の条件: なし'), null);
  assert.equal(parseBacklog('担当: role:ux\n着手の条件: なし'), null);
  assert.equal(parseBacklog('着手の条件: なし'), null);
});

test('closing keywords in a PR body', () => {
  assert.deepEqual(closingIssues('Closes #12\nRefs #13\nfixes: #14, resolved #15'), [12, 14, 15]);
  assert.deepEqual(closingIssues('Refs #3。other/repo#4 は別。 discloses #5'), []);
  assert.deepEqual(closingIssues(null), []);
});

// An in-memory GitHub with just what the sweep touches.
function fakeGitHub({ pulls = [], issues = [], paused = false, mainConclusion = 'success', failMerge = [] } = {}) {
  const state = { pulls, issues, wakes: [], dispatched: 0, mainTip: 'tip0', ciTips: new Set(['tip0']), mainRunId: 1 };
  const find = (n) => [...state.pulls, ...state.issues].find((x) => x.number === Number(n));
  const asIssue = (x) => ({ ...x, user: { login: x.author ?? OWNER }, labels: x.labels.map((name) => ({ name })) });
  const asPull = (p) => ({
    ...asIssue(p),
    head: { sha: p.sha, ref: p.head, repo: { full_name: p.fork ? 'stranger/openut' : REPO } },
    base: { ref: p.base },
  });
  const ciRuns = (sha) => {
    const p = state.pulls.find((x) => x.sha === sha);
    const kind = p?.ci ?? 'success';
    if (kind === 'none') return [];
    const base = { created_at: '2026-11-28T01:00:00Z', event: 'pull_request', html_url: `https://ci/run/${sha}` };
    if (kind === 'pending') return [{ ...base, status: 'in_progress', conclusion: null }];
    return [{ ...base, status: 'completed', conclusion: kind }];
  };
  const handle = (method, path, body) => {
    let m;
    if (path === '/contents/docs/project/PAUSED') return paused ? {} : null;
    if (path === '') return { default_branch: 'main' };
    if ((m = path.match(/^\/actions\/workflows\/ci\.yml\/runs\?head_sha=(\S+?)&branch=main/))) {
      return { workflow_runs: state.ciTips.has(m[1]) ? [{}] : [] };
    }
    if ((m = path.match(/^\/actions\/workflows\/ci\.yml\/runs\?head_sha=(\S+)$/))) return { workflow_runs: ciRuns(m[1]) };
    if (path.startsWith('/actions/workflows/ci.yml/runs?branch=main')) {
      return { workflow_runs: [{ id: state.mainRunId, event: 'push', conclusion: mainConclusion, html_url: 'https://ci/run' }] };
    }
    if (path === '/actions/workflows/ci.yml/dispatches') return void (state.dispatched++, state.ciTips.add(state.mainTip)) ?? {};
    if (path === '/commits/main') return { sha: state.mainTip };
    if ((m = path.match(/^\/commits\/([^/]+)$/))) {
      const ref = decodeURIComponent(m[1]);
      if (state.branchTips?.[ref]) return { sha: state.branchTips[ref] };
      const owners = state.pulls.filter((p) => p.head === ref);
      return owners.length ? { sha: owners.at(-1).sha } : null;
    }
    if (path.startsWith('/pulls?state=open')) return state.pulls.filter((p) => p.state === 'open').map(asPull);
    if ((m = path.match(/^\/pulls\?state=closed&head=owner:([^&]+)/))) {
      return state.pulls
        .filter((p) => p.state === 'closed' && p.head === decodeURIComponent(m[1]))
        .map((p) => ({ ...asPull(p), merged_at: p.merged ? 'now' : null }));
    }
    if (path.startsWith('/pulls?state=closed&base=main')) {
      return state.pulls
        .filter((p) => p.state === 'closed')
        .map((p) => ({ ...asPull(p), merged_at: p.merged ? (p.mergedAt ?? new Date().toISOString()) : null }));
    }
    if (path.startsWith('/issues?state=all')) return state.issues.map(asIssue);
    if (path.startsWith('/issues?state=open&labels=')) {
      const label = path.match(/labels=([^&]+)/)[1];
      return state.issues.filter((i) => i.state === 'open' && i.labels.includes(label)).map(asIssue);
    }
    if (method === 'POST' && path === '/issues') {
      const issue = { number: 900 + state.issues.length, state: 'open', title: body.title, body: body.body, labels: body.labels, comments: [] };
      state.issues.push(issue);
      return asIssue(issue);
    }
    if ((m = path.match(/^\/pulls\/(\d+)\/files/))) return find(m[1]).files.map((filename) => ({ filename }));
    if ((m = path.match(/^\/pulls\/(\d+)\/merge$/))) {
      const p = find(m[1]);
      if (state.modifiedOnce?.includes(p.number)) {
        state.modifiedOnce.splice(state.modifiedOnce.indexOf(p.number), 1);
        throw new Error('405 Base branch was modified. Review and try the merge again.');
      }
      if (failMerge.includes(p.number) || body.sha !== p.sha) throw new Error('405 not allowed');
      Object.assign(p, { state: 'closed', merged: true, method: body.merge_method });
      state.mainTip = `tip-after-${p.number}`;
      return { merged: true };
    }
    if ((m = path.match(/^\/pulls\/(\d+)$/))) {
      const p = find(m[1]);
      if (!p || !p.files) return null;
      if (method === 'PATCH') return Object.assign(p, body) && {};
      return asPull(p);
    }
    if ((m = path.match(/^\/issues\/(\d+)\/comments/))) {
      const x = find(m[1]);
      if (method === 'POST') return void x.comments.push(comment(body.body, { slug: 'github-actions' }, 'github-actions[bot]')) ?? {};
      return x.comments;
    }
    if ((m = path.match(/^\/issues\/(\d+)\/labels\/(.+)$/))) {
      const x = find(m[1]);
      const name = decodeURIComponent(m[2]);
      if (!x.labels.includes(name)) return null; // GitHub answers 404 for a label that is not there
      x.labels = x.labels.filter((l) => l !== name);
      return {};
    }
    if ((m = path.match(/^\/issues\/(\d+)\/labels$/))) return void find(m[1]).labels.push(...body.labels) ?? {};
    if ((m = path.match(/^\/issues\/(\d+)\/events/))) return find(m[1]).events ?? [];
    if ((m = path.match(/^\/issues\/(\d+)$/))) {
      const x = find(m[1]);
      if (!x) return null;
      if (method === 'PATCH') return Object.assign(x, body) && {};
      return x.files ? { ...asIssue(x), pull_request: { merged_at: x.merged ? 'now' : null } } : asIssue(x);
    }
    throw new Error(`unhandled ${method} ${path}`);
  };
  const call = async (method, path, body) => {
    const data = handle(method, path, body);
    return data === null ? { ok: false, status: 404, data: null } : { ok: true, status: 200, data };
  };
  const gh = {
    call,
    get: (path) => call('GET', path),
    must: async (method, path, body) => {
      const data = handle(method, path, body);
      if (data === null) throw new Error(`404 ${path}`);
      return data;
    },
    list: async (path) => {
      const data = handle('GET', path);
      return Array.isArray(data) ? data : (data.check_runs ?? data.workflow_runs);
    },
    wake: async (wakePr, line) => void state.wakes.push(`${wakePr}: ${line}`),
  };
  return { gh, state, find };
}

const approveBy = (app, sha) => comment(`REVIEW: APPROVE\n\n対象: \`${sha.slice(0, 7)}\``, app);
const pull = (number, extra = {}) => ({
  number, state: 'open', draft: false, merged: false, mergeable: true, body: 'Closes #1000', labels: ['role:pm'],
  sha: `${number}`.repeat(8).slice(0, 8), head: `feat/${number}`, base: 'main', files: ['src/openut/a.py'], comments: [], ...extra,
});
const approved = (number, extra = {}) => {
  const p = pull(number, extra);
  p.comments.push(approveBy(requiredReviewerApp(p.files) === 'claude' ? viaClaude : viaChatgpt, p.sha));
  return p;
};
const issue = (number, body, labels = ['backlog'], state = 'open', extra = {}) => ({ number, state, title: `Issue ${number}`, body, labels, comments: [], ...extra });
const sweep = (world, extra = {}) => run({ gh: world.gh, repo: REPO, wakePr: WAKE_PR, log: () => {}, sleep: async () => {}, ...extra });

test('the number of the wake PR is required', async () => {
  await assert.rejects(run({ gh: fakeGitHub().gh, repo: REPO, wakePr: NaN, log: () => {} }), /wakePr/);
});

test('merges an approved PR whose CI passed, then the PR stacked on it, and releases what they unblock', async () => {
  const lower = approved(46);
  const upper = approved(49, { base: 'feat/46' });
  const world = fakeGitHub({
    pulls: [lower, upper],
    issues: [
      issue(52, '担当: role:impl\n着手の条件: #46 がマージ済み、#49 がマージ済み'),
      issue(54, '担当: role:test\n着手の条件: #53 が閉じている'),
      issue(53, 'M1', ['role:impl']),
    ],
  });
  await sweep(world);
  assert.deepEqual([lower.merged, lower.method, upper.merged, upper.base], [true, 'merge', true, 'main']);
  assert.deepEqual(lower.labels, []);
  assert.match(lower.comments.at(-1).body, /マージしました/);
  assert.deepEqual(world.find(52).labels, ['role:impl']);
  assert.deepEqual(world.find(54).labels, ['backlog']);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:impl #52']);
  // A merge made by the job does not start CI on main by itself: the sweep starts it, once.
  assert.equal(world.state.dispatched, 1);
});

test('documents are squashed; a design PR needs the ChatGPT side', async () => {
  const design = approved(7, { files: ['docs/design/overview.md'] });
  const wrongSide = pull(8, { files: ['docs/design/stage1.md'] });
  wrongSide.comments.push(approveBy(viaClaude, wrongSide.sha));
  const world = fakeGitHub({ pulls: [design, wrongSide] });
  await sweep(world);
  assert.deepEqual([design.merged, design.method], [true, 'squash']);
  assert.equal(wrongSide.merged, false);
});

test('does not merge without approval of the head, before CI finishes, while held, a draft, a fork, or when paused', async () => {
  const stale = pull(11, { comments: [approveBy(viaClaude, 'ffffffff')] });
  const otherSide = pull(12);
  otherSide.comments.push(approveBy(viaChatgpt, otherSide.sha));
  const running = approved(13, { ci: 'pending' });
  const noRun = approved(14, { ci: 'none' });
  const cancelled = approved(15, { ci: 'cancelled' });
  const held = approved(16, { files: ['docs/design/a.md'], body: 'マージの条件: オーナーの了承' });
  const draft = approved(17, { draft: true });
  const fork = approved(18, { fork: true });
  const stranger = pull(19);
  stranger.comments.push(comment(`REVIEW: APPROVE\n対象: ${stranger.sha}`, null, 'stranger'));
  const all = [stale, otherSide, running, noRun, cancelled, held, draft, fork, stranger];
  const world = fakeGitHub({ pulls: all });
  await sweep(world);
  assert.deepEqual(all.map((p) => p.merged), all.map(() => false));
  // Waiting is silent: nobody is blamed and nothing is commented.
  assert.deepEqual(all.map((p) => p.comments.filter((c) => c.user.login === 'github-actions[bot]').length), all.map(() => 0));
  assert.deepEqual(world.state.wakes, []);
  assert.equal(world.state.dispatched, 0);

  const ok = approved(20);
  const pausedWorld = fakeGitHub({ pulls: [ok], paused: true, issues: [issue(21, '担当: role:impl\n着手の条件: なし')] });
  await sweep(pausedWorld);
  assert.equal(ok.merged, false);
  assert.deepEqual(pausedWorld.find(21).labels, ['backlog']);
});

test('a failed CI sends the PR back to its author once', async () => {
  const red = approved(3, { ci: 'failure', labels: [] });
  const tests = approved(4, { ci: 'failure', files: ['tests/test_replay.py'], labels: [] });
  const world = fakeGitHub({ pulls: [red, tests] });
  await sweep(world);
  await sweep(world);
  assert.deepEqual([red.merged, red.labels], [false, ['role:impl']]);
  assert.deepEqual([tests.merged, tests.labels], [false, ['role:test']]);
  assert.equal(red.comments.filter((c) => /CI が通っていません/.test(c.body)).length, 1);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:impl #3', '1: WAKE role:test #4']);
});

test('a conflict goes back to the author once; a refused merge goes to the PM once, without a wake', async () => {
  const conflict = approved(5, { mergeable: false, labels: [] });
  const workflow = approved(6, { files: ['.github/workflows/ci.yml'], labels: [] });
  const world = fakeGitHub({ pulls: [conflict, workflow], failMerge: [6] });
  await sweep(world);
  await sweep(world);
  assert.deepEqual(conflict.labels, ['role:impl']);
  assert.equal(conflict.comments.filter((c) => /競合/.test(c.body)).length, 1);
  assert.deepEqual(workflow.labels, ['role:pm']);
  assert.equal(workflow.comments.filter((c) => /PM がマージしてください/.test(c.body)).length, 1);
  // The PM runs once a day and has no wake.
  assert.deepEqual(world.state.wakes, ['1: WAKE role:impl #5']);
});

test('a merge refused because a branch just moved is retried', async () => {
  const p = approved(9);
  const world = fakeGitHub({ pulls: [p] });
  world.state.modifiedOnce = [9];
  await sweep(world);
  assert.equal(p.merged, true);
});

test('a red main stops merges and asks the implementer once', async () => {
  const p = approved(30);
  const world = fakeGitHub({ pulls: [p], mainConclusion: 'failure' });
  await sweep(world);
  await sweep(world);
  assert.equal(p.merged, false);
  const raised = world.state.issues.filter((i) => i.title.startsWith('main の CI が失敗している'));
  assert.equal(raised.length, 1);
  assert.deepEqual(raised[0].labels, ['role:impl']);
  assert.deepEqual(world.state.wakes, [`1: WAKE role:impl #${raised[0].number}`]);
  // Main has not moved past the failed run, so CI is not started again.
  assert.equal(world.state.dispatched, 0);
  // Once the fix is on main (merged by the PM), CI is started there.
  world.state.mainTip = 'tip-fixed';
  await sweep(world);
  assert.equal(world.state.dispatched, 1);
});

test('a merge closes the Issues the PR completes, and releases the backlog that waited for them', async () => {
  const p = approved(60, { body: '## 目的\nCloses #61\nRefs #62' });
  const world = fakeGitHub({
    pulls: [p],
    issues: [
      issue(61, '実装の依頼', ['role:impl']),
      issue(62, '別の依頼', ['role:impl']),
      issue(63, '担当: role:test\n着手の条件: #61 が閉じている'),
    ],
  });
  await sweep(world);
  assert.deepEqual([world.find(61).state, world.find(61).labels], ['closed', []]);
  assert.equal(world.find(62).state, 'open');
  assert.deepEqual(world.find(63).labels, ['role:test']);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:test #63']);
});

test('Issues of PRs merged earlier are closed by a later sweep, unless they were opened again', async () => {
  const merged = pull(70, { state: 'closed', merged: true, mergedAt: new Date(Date.now() - 3600e3).toISOString(), body: 'Closes #71\nCloses #72' });
  const world = fakeGitHub({
    pulls: [merged],
    issues: [
      issue(71, 'x', []),
      issue(72, 'y', [], 'open', { events: [{ event: 'reopened', created_at: new Date().toISOString() }] }),
    ],
  });
  await sweep(world);
  assert.equal(world.find(71).state, 'closed');
  assert.equal(world.find(72).state, 'open');
});

test('backlog release is capped by open implementation PRs and per role, and skips what the PM must look at', async () => {
  const open = [pull(81), pull(82), pull(83)];
  const world = fakeGitHub({
    pulls: open,
    issues: [
      issue(84, '担当: role:impl\n着手の条件: なし'),
      issue(85, '担当: role:design\n着手の条件: なし'),
      issue(86, '担当: role:design\n着手の条件: なし'),
      issue(87, '担当: role:design\n着手の条件: なし'),
      issue(88, '担当: role:test\n着手の条件: オーナーの返事のあと'),
      issue(89, '担当: role:test\n着手の条件: なし', ['backlog'], 'open', { author: 'stranger' }),
    ],
  });
  await sweep(world);
  assert.deepEqual(world.find(84).labels, ['backlog']); // three implementation PRs are open
  assert.deepEqual([85, 86, 87].map((n) => world.find(n).labels[0]), ['role:design', 'role:design', 'backlog']);
  assert.deepEqual(world.find(88).labels, ['backlog']);
  assert.deepEqual(world.find(89).labels, ['backlog']);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:design #85', '1: WAKE role:design #86']);
});

test('a condition is met only by work that was done: not by "not planned" or an unmerged PR', async () => {
  const closedUnmerged = pull(90, { state: 'closed', merged: false });
  const world = fakeGitHub({
    pulls: [closedUnmerged],
    issues: [
      issue(91, 'x', [], 'closed', { state_reason: 'not_planned' }),
      issue(92, '担当: role:impl\n着手の条件: #91 が閉じている'),
      issue(93, '担当: role:impl\n着手の条件: #90 がマージ済み'),
      issue(94, '担当: role:impl\n着手の条件: #90 が閉じている'),
    ],
  });
  await sweep(world);
  assert.deepEqual([92, 93, 94].map((n) => world.find(n).labels), [['backlog'], ['backlog'], ['backlog']]);
});

test('a stacked PR is retargeted only when its base branch is exactly what was merged into main', async () => {
  const lower = pull(100, { state: 'closed', merged: true, head: 'feat/lower', sha: 'aaaa1111' });
  const upper = approved(101, { base: 'feat/lower' });
  const world = fakeGitHub({ pulls: [lower, upper] });
  world.state.branchTips = { 'feat/lower': 'bbbb2222' }; // the branch moved after the merge
  await sweep(world);
  assert.deepEqual([upper.base, upper.merged], ['feat/lower', false]);
  world.state.branchTips = { 'feat/lower': 'aaaa1111' };
  await sweep(world);
  assert.deepEqual([upper.base, upper.merged], ['main', true]);
});
