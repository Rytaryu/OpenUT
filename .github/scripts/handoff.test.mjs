import assert from 'node:assert/strict';
import test from 'node:test';
import {
  authorRole,
  ciState,
  closingIssues,
  confirmationLinks,
  isApprovalOf,
  isHeld,
  isImplementationPr,
  latestReview,
  linkedCommentIds,
  mergeMethod,
  parseBacklog,
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

test('merge method and author role follow the changed paths', () => {
  assert.equal(mergeMethod(['docs/design/a.md', 'docs/research/b.md']), 'squash');
  assert.equal(mergeMethod(['docs/a.md', 'src/lib.rs']), 'merge');
  assert.equal(authorRole(['docs/design/ui/x.css']), 'role:ux');
  assert.equal(authorRole(['docs/design/ui/x.css', 'docs/design/a.md']), 'role:design');
  assert.equal(authorRole(['src/lib.rs']), 'role:impl');
  assert.equal(authorRole(['src/lib.rs', 'tests/replay.rs']), 'role:impl');
  assert.equal(authorRole(['tests/replay.rs']), 'role:impl');
  assert.equal(authorRole(['Cargo.toml', 'Cargo.lock']), 'role:impl');
  assert.equal(authorRole([]), 'role:impl');
  // The process documents, the research notes and the hand-off belong to the PM.
  assert.equal(authorRole(['README.md', 'AGENTS.md', 'docs/research/findings.md']), 'role:pm');
  assert.equal(authorRole(['.github/workflows/ci.yml', '.github/scripts/handoff.mjs']), 'role:pm');
  assert.equal(authorRole(['README.md', 'src/lib.rs']), 'role:impl');
  assert.equal(authorRole(['docs/design/a.md', 'docs/project/STATUS.md']), 'role:pm');
  // A file moved out of src/ into docs/design/ is still an implementation change.
  const moved = touchedPaths([{ filename: 'docs/design/a.md', previous_filename: 'src/lib.rs' }]);
  assert.equal(authorRole(moved), 'role:impl');
  // Only PRs of the implementer count toward the cap on open implementation PRs.
  assert.equal(isImplementationPr(['docs/project/STATUS.md', 'docs/design/a.md']), false);
  assert.equal(isImplementationPr(['.github/workflows/ci.yml']), false);
  assert.equal(isImplementationPr(['src/lib.rs', 'docs/a.md']), true);
});

test('only the reviewer (or the owner) can approve, and only the head commit', () => {
  const approve = `REVIEW: APPROVE\n\n対象: \`${SHA.slice(0, 7)}\``;
  // The Claude side (PM, design, UI/UX) cannot approve anything.
  assert.equal(latestReview([comment(approve, viaClaude)], OWNER), null);
  assert.ok(isApprovalOf(latestReview([comment(approve, viaChatgpt)], OWNER), SHA));
  assert.ok(isApprovalOf(latestReview([comment(approve)], OWNER), SHA));
  // Someone else's account never counts (the repository is public).
  assert.equal(latestReview([comment(approve, null, 'stranger')], OWNER), null);
  assert.equal(latestReview([comment(approve, viaChatgpt, 'stranger')], OWNER), null);
  // A later REQUEST_CHANGES wins; an approval of an older commit does not count.
  const later = [comment(approve, viaChatgpt), comment('REVIEW: REQUEST_CHANGES\n対象: abcdef1', viaChatgpt)];
  assert.equal(isApprovalOf(latestReview(later, OWNER), SHA), false);
  assert.equal(isApprovalOf(comment('REVIEW: APPROVE\n対象: `1111111`', viaChatgpt), SHA), false);
  // Only the first SHA on the 対象 line counts; SHAs elsewhere do not.
  assert.equal(isApprovalOf(comment(`REVIEW: APPROVE\n対象: 1111111\n前回（${SHA.slice(0, 7)}）の [必須] は直った`, viaChatgpt), SHA), false);
  assert.equal(isApprovalOf(comment(`REVIEW: APPROVE\n\n${SHA.slice(0, 7)} を見た`, viaChatgpt), SHA), false);
  assert.ok(isApprovalOf(comment(`REVIEW: APPROVE\r\n対象： ${SHA}`, viaChatgpt), SHA));
  // "REVIEW:" must start the comment; a quoted mention is not a review.
  assert.equal(latestReview([comment(`上の REVIEW: APPROVE は ${SHA}`, viaChatgpt)], OWNER), null);
});

test('a マージの条件 line holds the merge until it is confirmed', () => {
  const ctx = { owner: OWNER, repo: REPO };
  const held = (body, comments, extra = {}) => isHeld({ body, created_at: '2026-11-28T00:00:00Z' }, comments, { ...ctx, ...extra });
  assert.equal(held('説明', []), false);
  assert.equal(held('マージの条件: 設計の PR #5 が先に入っていること', []), true);
  assert.equal(held('', [comment('前置き\nマージの条件： #5 のあと', viaChatgpt)]), true);
  assert.equal(held('', [comment('マージの条件: A', viaChatgpt), comment('確認しました。\nマージの条件を確認: OK', viaClaude)]), false);
  assert.equal(held('', [comment('マージの条件を確認: OK', viaClaude), comment('マージの条件: B', viaClaude)]), true);
  assert.equal(held('', [comment('- `マージの条件:` の行はない'), comment('| `マージの条件:` | OK |')]), false);
});

test('only the owner\'s account sets or lifts a hold, and a hold on the owner\'s consent needs the owner\'s own word', () => {
  const ctx = { owner: OWNER, repo: REPO };
  const at = (c, created_at, id) => ({ ...c, created_at, id });
  const pr = { body: '説明', created_at: '2026-11-28T00:00:00Z' };
  const ownerHold = { body: 'マージの条件: オーナーの了承（公開の範囲）', created_at: '2026-11-28T00:00:00Z' };
  const link = (id, n = 7) => `https://github.com/${REPO}/issues/${n}#issuecomment-${id}`;

  // Someone else's comment neither holds nor lifts (the repository is public).
  assert.equal(isHeld(pr, [comment('マージの条件: 止めて', null, 'stranger')], ctx), false);
  assert.equal(isHeld({ ...pr, body: 'マージの条件: A' }, [comment('マージの条件を確認: OK', null, 'stranger')], ctx), true);
  assert.equal(isHeld(ownerHold, [comment('マージの条件を確認: OK', null, 'stranger')], ctx), true);

  // A role (any app) cannot lift a hold on the owner's consent by saying OK...
  assert.equal(isHeld(ownerHold, [comment('マージの条件を確認: OK', viaChatgpt)], ctx), true);
  assert.equal(isHeld(ownerHold, [comment('マージの条件を確認: OK', viaClaude)], ctx), true);
  // ...nor by linking to a comment that is not the owner's own word.
  const confirm = comment(`マージの条件を確認: OK\n${link(501)}`, viaChatgpt);
  assert.equal(isHeld(ownerHold, [confirm], ctx), true);
  assert.deepEqual(confirmationLinks([confirm, comment(`参考: ${link(777)}`, viaClaude)], ctx), [501]);
  // The owner's own word, given after the hold, lifts it; an older one does not.
  const after = new Map([[501, '2026-11-28T03:00:00Z']]);
  const before = new Map([[501, '2026-11-27T03:00:00Z']]);
  assert.equal(isHeld(ownerHold, [confirm], { ...ctx, ownerWords: after }), false);
  assert.equal(isHeld(ownerHold, [confirm], { ...ctx, ownerWords: before }), true);
  // The owner writing the confirmation without an app lifts it directly.
  assert.equal(isHeld(ownerHold, [comment('マージの条件を確認: OK')], ctx), false);
  // A hold set in a comment is timed from that comment.
  const later = at(comment('マージの条件: オーナーの了承', viaChatgpt), '2026-11-28T05:00:00Z', 9);
  assert.equal(isHeld(pr, [later, confirm], { ...ctx, ownerWords: after }), true);
  assert.equal(isHeld(pr, [later, confirm], { ...ctx, ownerWords: new Map([[501, '2026-11-28T06:00:00Z']]) }), false);

  // Links count only when they point into this repository.
  assert.deepEqual(linkedCommentIds(`${link(1)} https://github.com/other/repo/issues/7#issuecomment-2 https://github.com/${REPO.toUpperCase()}/pull/3#issuecomment-3`, REPO), [1, 3]);
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
  assert.deepEqual(parseBacklog('担当: role:design\r\n着手の条件: #53 が閉じている\r\n'), {
    role: 'role:design',
    conditions: [{ number: 53, kind: 'closed' }],
  });
  assert.deepEqual(parseBacklog('担当: role:impl\n着手の条件: #44 が閉じている、#14 がマージ済み').conditions, [
    { number: 44, kind: 'closed' },
    { number: 14, kind: 'merged' },
  ]);
  assert.equal(parseBacklog('担当: role:impl\n着手の条件: オーナーが了承したら'), null);
  assert.equal(parseBacklog('担当: role:pm\n着手の条件: なし'), null);
  assert.deepEqual(parseBacklog('担当: role:ux\n着手の条件: なし'), { role: 'role:ux', conditions: [] });
  assert.equal(parseBacklog('担当: role:review\n着手の条件: なし'), null);
  assert.equal(parseBacklog('担当: role:test\n着手の条件: なし'), null);
  assert.equal(parseBacklog('着手の条件: なし'), null);
});

test('closing keywords in a PR body', () => {
  assert.deepEqual(closingIssues('Closes #12\nRefs #13\nfixes: #14, resolved #15'), [12, 14, 15]);
  assert.deepEqual(closingIssues('Refs #3。other/repo#4 は別。 discloses #5'), []);
  assert.deepEqual(closingIssues(null), []);
});

// An in-memory GitHub with just what the sweep touches.
function fakeGitHub({ pulls = [], issues = [], paused = false, mainConclusion = 'success', failMerge = [] } = {}) {
  const state = { pulls, issues, wakes: [], dispatched: 0, mainTip: 'tip0', ciTips: new Set(['tip0']), mainRunId: 1, mainConclusion, failWakes: 0 };
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
      return { workflow_runs: [{ id: state.mainRunId, event: 'push', conclusion: state.mainConclusion, html_url: 'https://ci/run' }] };
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
    if ((m = path.match(/^\/issues\/comments\/(\d+)$/))) {
      return [...state.pulls, ...state.issues].flatMap((x) => x.comments).find((c) => c.id === Number(m[1])) ?? null;
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
    wake: async (wakePr, line) => {
      if (state.failWakes > 0) {
        state.failWakes--;
        throw new Error('POST /issues/1/comments -> 502');
      }
      state.wakes.push(`${wakePr}: ${line}`);
    },
  };
  return { gh, state, find };
}

const approveBy = (app, sha) => comment(`REVIEW: APPROVE\n\n対象: \`${sha.slice(0, 7)}\``, app);
const pull = (number, extra = {}) => ({
  number, state: 'open', draft: false, merged: false, mergeable: true, body: 'Closes #1000', labels: ['role:pm'],
  sha: `${number}`.repeat(8).slice(0, 8), head: `feat/${number}`, base: 'main', files: ['src/lib.rs'], comments: [], ...extra,
});
const approved = (number, extra = {}) => {
  const p = pull(number, extra);
  p.comments.push(approveBy(viaChatgpt, p.sha));
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
      issue(54, '担当: role:design\n着手の条件: #53 が閉じている'),
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

test('documents are squashed; the Claude side cannot approve', async () => {
  const design = approved(7, { files: ['docs/design/overview.md'] });
  const wrongSide = pull(8, { files: ['docs/design/stage1.md'] });
  wrongSide.comments.push(approveBy(viaClaude, wrongSide.sha));
  const world = fakeGitHub({ pulls: [design, wrongSide] });
  await sweep(world);
  assert.deepEqual([design.merged, design.method], [true, 'squash']);
  assert.equal(wrongSide.merged, false);
});

test('does not merge without approval of the head, before CI finishes, while held, a draft, a fork, or when paused', async () => {
  const stale = pull(11, { comments: [approveBy(viaChatgpt, 'ffffffff')] });
  const otherSide = pull(12);
  otherSide.comments.push(approveBy(viaClaude, otherSide.sha));
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
  const design = approved(4, { ci: 'failure', files: ['docs/design/a.md'], labels: [] });
  const world = fakeGitHub({ pulls: [red, design] });
  await sweep(world);
  await sweep(world);
  assert.deepEqual([red.merged, red.labels], [false, ['role:impl']]);
  assert.deepEqual([design.merged, design.labels], [false, ['role:design']]);
  assert.equal(red.comments.filter((c) => /CI が通っていません/.test(c.body)).length, 1);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:impl #3', '1: WAKE role:design #4']);
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
      issue(63, '担当: role:ux\n着手の条件: #61 が閉じている'),
    ],
  });
  await sweep(world);
  assert.deepEqual([world.find(61).state, world.find(61).labels], ['closed', []]);
  assert.equal(world.find(62).state, 'open');
  assert.deepEqual(world.find(63).labels, ['role:ux']);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:ux #63']);
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
      issue(88, '担当: role:ux\n着手の条件: オーナーの返事のあと'),
      issue(89, '担当: role:ux\n着手の条件: なし', ['backlog'], 'open', { author: 'stranger' }),
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

test('a hold on the owner\'s consent is lifted in a sweep only through the owner\'s own comment', async () => {
  const hold = { body: 'マージの条件: オーナーの了承', created_at: '2026-11-28T00:00:00Z' };
  const link = (id) => `https://github.com/${REPO}/issues/200#issuecomment-${id}`;
  const stamped = (c, id, created_at) => ({ ...c, id, created_at });
  const byStranger = approved(201, hold);
  byStranger.comments.push(comment('マージの条件を確認: OK', null, 'stranger'));
  const byRoleAlone = approved(202, hold);
  byRoleAlone.comments.push(comment('マージの条件を確認: OK', viaChatgpt));
  const linkedToRole = approved(203, hold);
  linkedToRole.comments.push(comment(`マージの条件を確認: OK\n${link(902)}`, viaChatgpt));
  const linkedToOwner = approved(204, hold);
  linkedToOwner.comments.push(comment(`マージの条件を確認: OK\n${link(901)}`, viaChatgpt));
  const linkedToStranger = approved(205, hold);
  linkedToStranger.comments.push(comment(`マージの条件を確認: OK\n${link(903)}`, viaChatgpt));
  const ask = issue(200, 'オーナーへの確認', ['needs-human']);
  ask.comments.push(
    stamped(comment('了承します'), 901, '2026-11-28T02:00:00Z'),
    stamped(comment('了承が出ました', viaClaude), 902, '2026-11-28T02:05:00Z'),
    stamped(comment('了承します', null, 'stranger'), 903, '2026-11-28T02:06:00Z'),
  );
  const all = [byStranger, byRoleAlone, linkedToRole, linkedToOwner, linkedToStranger];
  const world = fakeGitHub({ pulls: all, issues: [ask] });
  await sweep(world);
  assert.deepEqual(all.map((p) => p.merged), [false, false, false, true, false]);
});

test('CI that passes when it is run again on the same commit lets the merge go on', async () => {
  const p = approved(210, { ci: 'failure', labels: [] });
  const world = fakeGitHub({ pulls: [p] });
  await sweep(world);
  await sweep(world);
  assert.equal(p.merged, false);
  assert.equal(p.comments.filter((c) => /CI が通っていません/.test(c.body)).length, 1);
  p.ci = 'success'; // the same SHA, run again
  p.comments.push(approveBy(viaChatgpt, p.sha));
  await sweep(world);
  assert.deepEqual([p.merged, p.labels], [true, []]);
});

test('a marker written by someone else does not silence a notice', async () => {
  const p = approved(215, { ci: 'failure', labels: [] });
  p.comments.push(comment(`<!-- handoff:ci-failed ${p.sha} -->`, null, 'stranger'));
  const world = fakeGitHub({ pulls: [p] });
  await sweep(world);
  assert.equal(p.comments.filter((c) => /CI が通っていません/.test(c.body)).length, 1);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:impl #215']);
});

test('a wake that could not be posted is sent by the next sweep: failed CI, conflict, backlog, red main', async () => {
  const red = approved(220, { ci: 'failure', labels: [] });
  let world = fakeGitHub({ pulls: [red] });
  world.state.failWakes = 1;
  await assert.rejects(sweep(world), /502/);
  assert.deepEqual(world.state.wakes, []);
  await sweep(world);
  await sweep(world);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:impl #220']);
  assert.deepEqual(red.labels, ['role:impl']);
  assert.equal(red.comments.filter((c) => /CI が通っていません/.test(c.body)).length, 1);

  const conflict = approved(221, { mergeable: false, labels: [] });
  world = fakeGitHub({ pulls: [conflict] });
  world.state.failWakes = 1;
  await assert.rejects(sweep(world), /502/);
  await sweep(world);
  await sweep(world);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:impl #221']);
  assert.equal(conflict.comments.filter((c) => /競合/.test(c.body)).length, 1);

  // Backlog: the Issue keeps `backlog` until the wake is out, so it is found again
  // (even when the caps would now hold it back).
  world = fakeGitHub({
    pulls: [pull(231), pull(232)],
    issues: [issue(230, '担当: role:impl\n着手の条件: なし')],
  });
  world.state.failWakes = 1;
  await assert.rejects(sweep(world), /502/);
  assert.deepEqual(world.find(230).labels, ['backlog', 'role:impl']);
  world.state.pulls.push(pull(233)); // three implementation PRs are open by now
  await sweep(world);
  await sweep(world);
  assert.deepEqual(world.find(230).labels, ['role:impl']);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:impl #230']);
  assert.equal(world.find(230).comments.filter((c) => /担当に渡しました/.test(c.body)).length, 1);

  world = fakeGitHub({ mainConclusion: 'failure' });
  world.state.failWakes = 1;
  await assert.rejects(sweep(world), /502/);
  await sweep(world);
  await sweep(world);
  const raised = world.state.issues.filter((i) => i.title.startsWith('main の CI が失敗している'));
  assert.equal(raised.length, 1);
  assert.deepEqual(world.state.wakes, [`1: WAKE role:impl #${raised[0].number}`]);
});

test('the red-main Issue is closed when CI on main passes again, and the next failure raises a new one', async () => {
  const p = approved(240);
  // An Issue by someone else with the same title neither stands in for ours nor gets closed.
  const lookalike = issue(241, '<!-- handoff:red-main 1 -->', [], 'open', { author: 'stranger', title: 'main の CI が失敗している' });
  const world = fakeGitHub({ pulls: [p], issues: [lookalike], mainConclusion: 'failure' });
  await sweep(world);
  const raised = () => world.state.issues.filter((i) => i.number >= 900);
  assert.equal(raised().length, 1);
  assert.equal(p.merged, false);

  world.state.mainConclusion = 'success';
  world.state.mainRunId = 2;
  await sweep(world);
  assert.deepEqual([raised()[0].state, raised()[0].labels], ['closed', []]);
  assert.match(raised()[0].comments.at(-1).body, /CI が通ったので、閉じました/);
  assert.equal(lookalike.state, 'open');
  assert.equal(p.merged, true);

  world.state.mainConclusion = 'failure';
  world.state.mainRunId = 3;
  await sweep(world);
  assert.equal(raised().length, 2);
  assert.equal(raised()[1].state, 'open');
});
