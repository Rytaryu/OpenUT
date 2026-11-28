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
  run,
  touchedPaths,
} from './handoff.mjs';

const OWNER = 'owner';
const REPO = `${OWNER}/openut`;
const WAKE_PR = 1;
const SHA = 'abcdef1234567890';
const viaClaude = { slug: 'claude' };
const viaChatgpt = { slug: 'chatgpt-codex-connector' };
// Every comment made here is a second later than the one before, like comments on GitHub.
let clock = Date.parse('2026-11-28T00:00:00Z');
const tick = () => new Date((clock += 1000)).toISOString();
const comment = (body, app = null, login = OWNER) => {
  const created_at = tick();
  return { body, user: { login }, performed_via_github_app: app, created_at, updated_at: created_at };
};

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

test('a マージの条件 line in a comment holds the merge until a later comment confirms it', () => {
  const held = (comments, body = '説明') => isHeld({ body }, comments, { owner: OWNER });
  assert.equal(held([]), false);
  assert.equal(held([comment('前置き\nマージの条件： #5 のあと', viaChatgpt)]), true);
  assert.equal(held([comment('マージの条件: A', viaChatgpt), comment('確認しました。\nマージの条件を確認: OK', viaClaude)]), false);
  assert.equal(held([comment('- `マージの条件:` の行はない'), comment('| `マージの条件:` | OK |')]), false);
  // A confirmation written before the hold lifts nothing, in whatever order the list comes.
  const early = comment('マージの条件を確認: OK', viaClaude);
  const late = comment('マージの条件: B', viaClaude);
  assert.equal(held([early, late]), true);
  assert.equal(held([late, early]), true);
  // Two holds need a confirmation after the later one.
  const first = comment('マージの条件: A', viaChatgpt);
  const ok = comment('マージの条件を確認: OK', viaClaude);
  const second = comment('マージの条件: B', viaChatgpt);
  assert.equal(held([first, ok, second]), true);
  assert.equal(held([first, ok, second, comment('マージの条件を確認: OK', viaClaude)]), false);
});

test('a hold in the PR body lasts as long as the line is there', () => {
  const body = '## 目的\nマージの条件: 設計の PR #5 が先に入っていること';
  // The time of an edit of the body cannot be read, so no comment lifts it: not a role's, not the owner's.
  assert.equal(isHeld({ body }, [], { owner: OWNER }), true);
  assert.equal(isHeld({ body }, [comment('マージの条件を確認: OK', viaClaude)], { owner: OWNER }), true);
  assert.equal(isHeld({ body }, [comment('マージの条件を確認: OK')], { owner: OWNER }), true);
  assert.equal(isHeld({ body: '## 目的\n（条件は確認済みで、行を消した）' }, [comment('マージの条件を確認: OK', viaClaude)], { owner: OWNER }), false);
});

test('only the owner\'s account sets or lifts a hold, and a hold on the owner\'s consent needs the owner in person', () => {
  const held = (comments) => isHeld({ body: '説明' }, comments, { owner: OWNER });
  const plain = () => comment('マージの条件: #5 のあと', viaChatgpt);
  const consent = () => comment('マージの条件: オーナーの了承（公開の範囲）', viaChatgpt);
  const ok = (app, login) => comment('マージの条件を確認: OK', app, login);

  // Someone else's comment neither holds nor lifts (the repository is public).
  assert.equal(held([comment('マージの条件: 止めて', null, 'stranger')]), false);
  assert.equal(held([plain(), ok(null, 'stranger')]), true);
  assert.equal(held([consent(), ok(null, 'stranger')]), true);

  // No role can lift a hold on the owner's consent, with or without a link to the owner's words elsewhere.
  assert.equal(held([consent(), ok(viaChatgpt)]), true);
  assert.equal(held([consent(), ok(viaClaude)]), true);
  const relayed = `マージの条件を確認: OK\nhttps://github.com/${REPO}/issues/7#issuecomment-501`;
  assert.equal(held([consent(), comment(relayed, viaChatgpt)]), true);
  // What the owner says about something else lifts nothing: only the confirmation line does.
  assert.equal(held([consent(), comment('了承します（別の件）')]), true);
  assert.equal(held([consent(), comment('LGTM')]), true);
  // The owner in person, on this PR, after the hold.
  assert.equal(held([consent(), ok(null)]), false);
  assert.equal(held([ok(null), consent()]), true);
  // The owner's confirmation also covers an ordinary hold; a role's covers only the ordinary one.
  assert.equal(held([plain(), consent(), ok(null)]), false);
  assert.equal(held([plain(), consent(), ok(viaClaude)]), true);

  // A hold that was edited after the confirmation (the condition changed) holds again.
  const changed = consent();
  const confirmed = ok(null);
  changed.updated_at = tick();
  assert.equal(held([changed, confirmed]), true);
  assert.equal(held([changed, confirmed, ok(null)]), false);
  // A comment that was edited into a confirmation does not count (anyone with write access can edit a comment).
  const doctored = comment('マージの条件を確認: OK');
  const h = consent();
  doctored.created_at = tick();
  doctored.updated_at = tick();
  assert.equal(held([h, doctored]), true);
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
  const state = { pulls, issues, wakes: [], dispatched: 0, mainTip: 'tip0', ciTips: new Set(['tip0']), mainRunId: 1, mainConclusion, failWakes: 0, failLabels: 0, failComments: 0 };
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
    if ((m = path.match(/^\/issues\/(\d+)\/comments/))) {
      const x = find(m[1]);
      if (method === 'POST') {
        if (state.failComments > 0) {
          state.failComments--;
          throw new Error(`POST /issues/${m[1]}/comments -> 502`);
        }
        return void x.comments.push(comment(body.body, { slug: 'github-actions' }, 'github-actions[bot]')) ?? {};
      }
      return x.comments;
    }
    if ((m = path.match(/^\/issues\/(\d+)\/labels\/(.+)$/))) {
      const x = find(m[1]);
      const name = decodeURIComponent(m[2]);
      if (!x.labels.includes(name)) return null; // GitHub answers 404 for a label that is not there
      x.labels = x.labels.filter((l) => l !== name);
      return {};
    }
    if ((m = path.match(/^\/issues\/(\d+)\/labels$/))) {
      const x = find(m[1]);
      if (state.failLabels > 0) {
        state.failLabels--;
        throw new Error(`POST /issues/${m[1]}/labels -> 502`);
      }
      x.labels.push(...body.labels);
      x.events = [...(x.events ?? []), ...body.labels.map((name) => ({ event: 'labeled', label: { name }, actor: { login: 'github-actions[bot]' }, created_at: tick() }))];
      return {};
    }
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

test('in a sweep, a hold on the owner\'s consent is lifted only by the owner in person, on that PR', async () => {
  const hold = () => comment('マージの条件: オーナーの了承', viaChatgpt);
  const make = (number, ...after) => {
    const p = pull(number);
    p.comments.push(hold());
    p.comments.push(approveBy(viaChatgpt, p.sha), ...after.map((f) => f()));
    return p;
  };
  const link = `https://github.com/${REPO}/issues/200#issuecomment-901`;
  const none = make(201);
  const byStranger = make(202, () => comment('マージの条件を確認: OK', null, 'stranger'));
  const byRole = make(203, () => comment('マージの条件を確認: OK', viaChatgpt));
  const relayed = make(204, () => comment(`マージの条件を確認: OK\n${link}`, viaChatgpt));
  const otherTopic = make(205, () => comment('了承します'));
  const inPerson = make(206, () => comment('マージの条件を確認: OK'));
  // The owner's consent on the needs-human Issue is not a confirmation on the PR.
  const ask = issue(200, 'オーナーへの確認', ['needs-human']);
  ask.comments.push({ ...comment('マージの条件を確認: OK'), id: 901 });
  const bodyHold = approved(207, { body: 'マージの条件: オーナーの了承' });
  bodyHold.comments.push(comment('マージの条件を確認: OK'));
  const all = [none, byStranger, byRole, relayed, otherTopic, inPerson, bodyHold];
  const world = fakeGitHub({ pulls: all, issues: [ask] });
  await sweep(world);
  assert.deepEqual(all.map((p) => p.merged), [false, false, false, false, false, true, false]);
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

test('taking a cut-short release up again never gives the label a second time', async () => {
  // The design role starts on its label, so it can work although the wake failed:
  // it opened a PR and took its label off the Issue before the next sweep.
  let world = fakeGitHub({ issues: [issue(300, '担当: role:design\n着手の条件: なし')] });
  world.state.failWakes = 1;
  await assert.rejects(sweep(world), /502/);
  assert.deepEqual(world.find(300).labels, ['backlog', 'role:design']);
  world.find(300).labels = ['backlog']; // handed on by the role
  await sweep(world);
  await sweep(world);
  assert.deepEqual(world.find(300).labels, []);
  assert.deepEqual(world.state.wakes, []);
  assert.equal(world.find(300).comments.filter((c) => /担当に渡しました/.test(c.body)).length, 1);

  // The role has started (its comment is there) but still holds the Issue: the wake is sent, the label is left alone.
  world = fakeGitHub({ issues: [issue(301, '担当: role:design\n着手の条件: なし')] });
  world.state.failWakes = 1;
  await assert.rejects(sweep(world), /502/);
  world.find(301).comments.push(comment('着手: 設計担当', viaClaude));
  await sweep(world);
  assert.deepEqual(world.find(301).labels, ['role:design']);
  assert.deepEqual(world.state.wakes, ['1: WAKE role:design #301']);
  assert.equal(world.find(301).events.filter((e) => e.event === 'labeled').length, 1);

  // The label could not be given at all: nothing was handed over, and the next sweep does all of it once.
  world = fakeGitHub({ issues: [issue(302, '担当: role:impl\n着手の条件: なし')] });
  world.state.failLabels = 1;
  await assert.rejects(sweep(world), /502/);
  assert.deepEqual([world.find(302).labels, world.state.wakes], [['backlog'], []]);
  await sweep(world);
  assert.deepEqual([world.find(302).labels, world.state.wakes], [['role:impl'], ['1: WAKE role:impl #302']]);

  // An Issue the PM put back into the backlog later is a new request.
  const again = issue(303, '担当: role:impl\n着手の条件: なし', ['backlog']);
  again.events = [
    { event: 'labeled', label: { name: 'role:impl' }, actor: { login: 'github-actions[bot]' }, created_at: tick() },
    { event: 'labeled', label: { name: 'backlog' }, actor: { login: OWNER }, created_at: tick() },
  ];
  world = fakeGitHub({ issues: [again] });
  await sweep(world);
  assert.deepEqual([again.labels, world.state.wakes], [['role:impl'], ['1: WAKE role:impl #303']]);

  // The same when the earlier label and the new request carry the same second: the order of the events decides.
  const sameSecond = issue(304, '担当: role:impl\n着手の条件: なし', ['backlog']);
  const at = tick();
  sameSecond.events = [
    { event: 'labeled', label: { name: 'backlog' }, actor: { login: OWNER }, created_at: at },
    { event: 'labeled', label: { name: 'role:impl' }, actor: { login: 'github-actions[bot]' }, created_at: at },
    { event: 'unlabeled', label: { name: 'role:impl' }, actor: { login: OWNER }, created_at: at },
    { event: 'labeled', label: { name: 'backlog' }, actor: { login: OWNER }, created_at: at },
  ];
  world = fakeGitHub({ issues: [sameSecond] });
  await sweep(world);
  assert.deepEqual([sameSecond.labels, world.state.wakes], [['role:impl'], ['1: WAKE role:impl #304']]);
});

test('taking a cut-short notice up again does not move a label the role has moved on', async () => {
  // CI failed on an approved design PR; the label was given, the wake failed.
  const p = approved(310, { ci: 'failure', files: ['docs/design/a.md'], labels: [] });
  let world = fakeGitHub({ pulls: [p] });
  world.state.failWakes = 1;
  await assert.rejects(sweep(world), /502/);
  assert.deepEqual(p.labels, ['role:design']);
  // The design role started on the label and passed the PR to the PM with a question, without a new commit.
  p.labels = ['role:pm'];
  await sweep(world);
  await sweep(world);
  assert.deepEqual(p.labels, ['role:pm']);
  assert.deepEqual(world.state.wakes, []);
  assert.equal(p.comments.filter((c) => /CI が通っていません/.test(c.body)).length, 1);

  // The notice itself could not be posted: it is posted by the next sweep, and the role is not woken twice for nothing.
  const q = approved(311, { ci: 'failure', labels: [] });
  world = fakeGitHub({ pulls: [q] });
  world.state.failComments = 1;
  await assert.rejects(sweep(world), /502/);
  assert.deepEqual([q.labels, world.state.wakes], [['role:impl'], ['1: WAKE role:impl #311']]);
  q.labels = ['role:review']; // fixed nothing yet, but moved on
  await sweep(world);
  assert.deepEqual([q.labels, world.state.wakes.length], [['role:review'], 1]);
  assert.equal(q.comments.filter((c) => /CI が通っていません/.test(c.body)).length, 1);

  // A label given before this approval (an earlier round) does not count as given now.
  const r = pull(312, { ci: 'failure', labels: [] });
  r.events = [{ event: 'labeled', label: { name: 'role:impl' }, actor: { login: 'github-actions[bot]' }, created_at: tick() }];
  r.comments.push(approveBy(viaChatgpt, r.sha));
  world = fakeGitHub({ pulls: [r] });
  await sweep(world);
  assert.deepEqual([r.labels, world.state.wakes], [['role:impl'], ['1: WAKE role:impl #312']]);
});
