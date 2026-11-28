#!/usr/bin/env node
// Mechanical hand-offs that must not wait for an AI role or the daily PM run
// (docs/project/roles/README.md, sections 6 and 7). No AI is involved.
//
//   1. Merge every open PR whose latest review is an APPROVE of its head commit
//      by the reviewer, once CI (ci.yml) has passed on that commit.
//   2. Close the Issues that the merged PRs complete ("Closes #N").
//   3. Release backlog Issues whose start conditions are now met.
//
// Each run is a full, idempotent sweep. A hand-off that was cut short (a label
// was changed but the wake comment could not be posted) is taken up again by
// the next sweep: the comment or label that marks a hand-off as done is always
// written last. Env: GITHUB_TOKEN, REPO (owner/name),
// WAKE_PR (the number of the wake PR), WAKE_TOKEN (optional; a token of the
// owner, so that wake comments start the ChatGPT tasks), CI_WORKFLOW (default
// ci.yml), DRY_RUN=true (print the writes instead of making them).
//
// Based on the hand-off of Rytaryu/Solfege, without its Actions budget and its
// check-after-approval: this repository is public, so CI runs on every PR (D-8).

export const SIGN = '— 受け渡しの仕組み（GitHub Actions。README の 6・7 章）';
export const RELEASE_ROLES = ['role:impl', 'role:design', 'role:ux'];
export const MAX_OPEN_IMPL_PRS = 3;
export const MAX_RELEASES_PER_ROLE = 2;
export const CLOSE_LOOKBACK_DAYS = 7;
// Every PR is reviewed by one role, the reviewer, which works through this app (D-18).
export const REVIEWER_APP = 'chatgpt-codex-connector';

const under = (files, dir) => files.length > 0 && files.every((f) => f.startsWith(dir));
const appOf = (comment) => comment.performed_via_github_app?.slug ?? null;
const firstLine = (text) => (text ?? '').replace(/\r/g, '').split('\n')[0].trim();

export function mergeMethod(files) {
  return under(files, 'docs/') ? 'squash' : 'merge';
}

// What the PM looks after: documents that belong to no other role (the process
// and product documents, the research notes, a mix of several kinds), the
// documents at the top of the repository, and the hand-off itself.
const PM_OWNED = /^(docs\/|\.github\/|[^/]+\.md$|LICENSE$)/;

/** The role that fixes this PR when it cannot be merged. */
export function authorRole(files) {
  if (under(files, 'docs/design/ui/')) return 'role:ux';
  if (under(files, 'docs/design/')) return 'role:design';
  if (files.length > 0 && files.every((f) => PM_OWNED.test(f))) return 'role:pm';
  return 'role:impl';
}

/**
 * The latest review that counts: a comment starting with "REVIEW:" written on
 * the owner's account, either by the owner (no app) or through the reviewer's
 * app. Comments through any other app are ignored, so the roles on the Claude
 * side (PM, design, UI/UX) cannot approve. The implementer works through the
 * same app as the reviewer and cannot be told apart here: not reviewing one's
 * own work is a rule the roles keep.
 */
export function latestReview(comments, owner) {
  const reviews = comments.filter(
    (c) => c.user?.login === owner && /^REVIEW:/.test(firstLine(c.body)) && [null, REVIEWER_APP].includes(appOf(c)),
  );
  return reviews.at(-1) ?? null;
}

/**
 * An approval must name the commit it approved: the first SHA (7 or more hex
 * digits) on its "対象:" line has to be a prefix of the head SHA. SHAs elsewhere
 * in the comment (an earlier review, the base) do not count.
 */
export function isApprovalOf(review, headSha) {
  if (!review) return false;
  const body = (review.body ?? '').replace(/\r/g, '');
  if (!/^REVIEW:\s*APPROVE\b/.test(firstLine(body))) return false;
  const target = body.match(/^\s*対象[:：](.*)$/m)?.[1] ?? '';
  const sha = target.match(/\b[0-9a-f]{7,40}\b/)?.[0];
  return Boolean(sha) && headSha.startsWith(sha);
}

/** Paths a PR touches, including the old path of a renamed file. */
export const touchedPaths = (files) => files.flatMap((f) => [f.filename, f.previous_filename].filter(Boolean));

/** PRs that count toward the cap on open implementation PRs. */
export const isImplementationPr = (paths) => authorRole(paths) === 'role:impl';

const HOLD = /^\s*マージの条件[:：](.*)$/m;
const CLEAR = /^\s*マージの条件を確認[:：]\s*OK/m;
const escapeRe = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** Ids of the comments of this repository that a text links to. */
export function linkedCommentIds(text, repo) {
  const link = new RegExp(`github\\.com/${escapeRe(repo)}/(?:issues|pull)/\\d+#issuecomment-(\\d+)`, 'gi');
  return [...(text ?? '').matchAll(link)].map((m) => Number(m[1]));
}

/**
 * A line starting with "マージの条件:" holds the merge until a later comment has
 * a line starting with "マージの条件を確認: OK".
 *
 * This repository is public, so the text alone decides nothing:
 *  - Only the PR body and comments written on the owner's account can set or
 *    lift a hold. Anyone else's comment changes nothing.
 *  - A hold whose line names the owner ("マージの条件: オーナーの了承") is lifted
 *    only by the owner's own word: a confirmation the owner wrote without an
 *    app, or a confirmation by a role that links to a comment the owner wrote
 *    without an app after the hold was set. `ownerWords` maps the ids of such
 *    comments (verified by the caller) to their creation time. Every role and
 *    this hand-off write through an app, so they cannot stand in for the owner.
 */
export function isHeld(pr, comments, { owner, repo, ownerWords = new Map() }) {
  const holdOf = (text, at) => {
    const m = (text ?? '').replace(/\r/g, '').match(HOLD);
    return m ? { needsOwner: /オーナー/.test(m[1]), at } : null;
  };
  let hold = holdOf(pr.body, pr.created_at);
  for (const c of comments) {
    if (c.user?.login !== owner) continue;
    const body = (c.body ?? '').replace(/\r/g, '');
    if (hold && CLEAR.test(body)) {
      const after = (at) => Date.parse(at) >= Date.parse(hold.at);
      const byOwner =
        appOf(c) === null || linkedCommentIds(body, repo).some((id) => ownerWords.has(id) && after(ownerWords.get(id)));
      if (!hold.needsOwner || byOwner) hold = null;
    }
    hold = holdOf(body, c.created_at) ?? hold;
  }
  return hold !== null;
}

/** Ids of the comments that the confirmations on a PR point to (to be verified as the owner's own). */
export function confirmationLinks(comments, { owner, repo }) {
  return [
    ...new Set(
      comments
        .filter((c) => c.user?.login === owner && CLEAR.test((c.body ?? '').replace(/\r/g, '')))
        .flatMap((c) => linkedCommentIds(c.body, repo)),
    ),
  ];
}

/**
 * The state of CI on a commit, from the runs of the CI workflow with that head
 * SHA: the newest run decides. 'success', 'failure', or 'pending' (no run yet,
 * still running, or cancelled: nothing to act on).
 */
export function ciState(runs) {
  const newest = [...runs].sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at))[0];
  if (!newest || newest.status !== 'completed') return { state: 'pending', run: newest ?? null };
  if (newest.conclusion === 'success') return { state: 'success', run: newest };
  if (['failure', 'timed_out', 'startup_failure'].includes(newest.conclusion)) return { state: 'failure', run: newest };
  return { state: 'pending', run: newest };
}

/** Parses the two header lines of a backlog Issue. Returns null when they are not machine-checkable. */
export function parseBacklog(body) {
  const text = (body ?? '').replace(/\r/g, '');
  const role = text.match(/^\s*担当[:：]\s*(role:[a-z-]+)\s*$/m)?.[1];
  const raw = text.match(/^\s*着手の条件[:：]\s*(.+)$/m)?.[1];
  if (!role || !RELEASE_ROLES.includes(role) || !raw) return null;
  const spec = raw.replace(/（[^）]*）/g, '').trim();
  if (spec === 'なし') return { role, conditions: [] };
  const conditions = [];
  for (const part of spec.split(/[、,]/).map((s) => s.trim()).filter(Boolean)) {
    const m = part.match(/^#(\d+)\s*が\s*(マージ済み|閉じている)$/);
    if (!m) return null;
    conditions.push({ number: Number(m[1]), kind: m[2] === 'マージ済み' ? 'merged' : 'closed' });
  }
  return conditions.length > 0 ? { role, conditions } : null;
}

/**
 * Issues of this repository that a PR completes: GitHub's closing keywords
 * ("Closes #N", "Fixes #N", "Resolves #N"...) in the PR body. "Refs #N" and
 * references to other repositories (owner/repo#N) are not included.
 */
export function closingIssues(body) {
  const numbers = new Set();
  const keyword = /(?<![\w-])(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?)[:：]?\s+#(\d+)\b/gi;
  for (const m of (body ?? '').matchAll(keyword)) numbers.add(Number(m[1]));
  return [...numbers];
}

const marker = (kind, id) => `<!-- handoff:${kind} ${id} -->`;

export function createClient({ repo, token, wakeToken, fetchImpl = fetch }) {
  const call = async (method, path, body, useToken = token) => {
    const url = path.startsWith('http') ? path : `https://api.github.com/repos/${repo}${path}`;
    const res = await fetchImpl(url, {
      method,
      headers: {
        authorization: `Bearer ${useToken}`,
        accept: 'application/vnd.github+json',
        'x-github-api-version': '2022-11-28',
        ...(body ? { 'content-type': 'application/json' } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
    });
    const text = await res.text();
    const data = text ? JSON.parse(text) : null;
    return { ok: res.ok, status: res.status, data };
  };
  const must = async (method, path, body, useToken) => {
    const r = await call(method, path, body, useToken);
    if (!r.ok) throw new Error(`${method} ${path} -> ${r.status} ${JSON.stringify(r.data)?.slice(0, 200)}`);
    return r.data;
  };
  const list = async (path) => {
    const out = [];
    for (let page = 1; ; page++) {
      const data = await must('GET', `${path}${path.includes('?') ? '&' : '?'}per_page=100&page=${page}`);
      const items = Array.isArray(data) ? data : (data.check_runs ?? data.workflow_runs ?? []);
      out.push(...items);
      if (items.length < 100) return out;
    }
  };
  return {
    call,
    get: (path) => call('GET', path),
    must,
    list,
    wake: (wakePr, line) => must('POST', `/issues/${wakePr}/comments`, { body: line }, wakeToken || token),
  };
}

const labelPath = (n, name) => `/issues/${n}/labels/${encodeURIComponent(name)}`;

/**
 * Leaves `role` as the only role label (none when null). A role may remove its
 * own label at the same moment, so a label that is already gone is fine.
 */
async function setRole(gh, number, labels, role) {
  for (const l of labels.filter((x) => x.startsWith('role:') || x === 'backlog')) {
    if (l === role) continue;
    const r = await gh.call('DELETE', labelPath(number, l));
    if (!r.ok && r.status !== 404) throw new Error(`DELETE label ${l} on #${number} -> ${r.status}`);
  }
  if (role && !labels.includes(role)) await gh.must('POST', `/issues/${number}/labels`, { labels: [role] });
}

const RED_MAIN_TITLE = 'main の CI が失敗している';

export async function run({
  gh,
  repo,
  wakePr,
  ciWorkflow = 'ci.yml',
  log = console.log,
  sleep = (ms) => new Promise((r) => setTimeout(r, ms)),
}) {
  if (!Number.isInteger(wakePr) || wakePr <= 0) throw new Error('wakePr (the number of the wake PR) is required');
  const owner = repo.split('/')[0];

  const paused = await gh.get('/contents/docs/project/PAUSED');
  if (paused.ok) return log('PAUSED: skip');
  if (paused.status !== 404) throw new Error(`cannot read PAUSED state (${paused.status})`);

  const main = (await gh.must('GET', '')).default_branch;

  // The PM has no event trigger (one run a day), so no wake comment for role:pm.
  const wake = async (role, n) => {
    if (role !== 'role:pm') await gh.wake(wakePr, `WAKE ${role} #${n}`);
  };

  // Written by this hand-off (the job's token) or on the owner's account.
  const trusted = (user) => user?.login === owner || user?.login === 'github-actions[bot]';

  // Do not pile merges on a red main. One Issue per failed run, whatever happens
  // to its labels; it is closed here once CI on main passes again.
  const lastMainRun = (
    await gh.must('GET', `/actions/workflows/${ciWorkflow}/runs?branch=${main}&status=completed&per_page=20`)
  ).workflow_runs.find((r) => r.event !== 'pull_request' && r.conclusion !== 'cancelled' && r.conclusion !== 'skipped');
  const mainRed = ['failure', 'timed_out', 'startup_failure'].includes(lastMainRun?.conclusion);
  const redMainIssues = (await gh.must('GET', '/issues?state=all&sort=created&direction=desc&per_page=100')).filter(
    (i) => !i.pull_request && trusted(i.user) && (i.body ?? '').includes('<!-- handoff:red-main '),
  );
  const openRedMain = redMainIssues.filter((i) => i.state === 'open');
  if (mainRed) {
    log(`main CI is failing (${lastMainRun.html_url}): no merges in this run`);
    // An open Issue covers every failed run until main is green again. An Issue
    // for this very run that someone closed is not raised a second time.
    let raised = openRedMain[0] ?? null;
    let fresh = false;
    if (!raised && !redMainIssues.some((i) => i.body.includes(marker('red-main', lastMainRun.id)))) {
      raised = await gh.must('POST', '/issues', {
        title: `${RED_MAIN_TITLE}（マージを止めています）`,
        labels: ['role:impl'],
        body: `${main} の CI が失敗しています: ${lastMainRun.html_url}\n\n直るまで、仕組みは PR をマージしません。実装担当は、原因を調べて直す PR を出してください（その PR は、レビューの承認のあと、PM が手でマージします）。${main} の CI が通ったら、仕組みがこの Issue を閉じます。\n\n${SIGN}\n${marker('red-main', lastMainRun.id)}`,
      });
      fresh = true;
    }
    // The wake is sent until the comment that records it exists, so a post
    // that failed is made good by the next sweep.
    if (raised) {
      const woken =
        !fresh &&
        (await gh.list(`/issues/${raised.number}/comments`)).some(
          (c) => trusted(c.user) && (c.body ?? '').includes(marker('woke', raised.number)),
        );
      if (!woken) {
        await wake('role:impl', raised.number);
        await gh.must('POST', `/issues/${raised.number}/comments`, {
          body: `実装担当に、起動の合図を送りました。\n\n${SIGN}\n${marker('woke', raised.number)}`,
        });
      }
    }
  } else if (lastMainRun?.conclusion === 'success') {
    for (const i of openRedMain) {
      await gh.must('POST', `/issues/${i.number}/comments`, {
        body: `${main} の CI が通ったので、閉じました: ${lastMainRun.html_url}\n\n仕組みは、PR のマージを再開します。\n\n${SIGN}`,
      });
      await setRole(gh, i.number, i.labels.map((l) => l.name), null);
      await gh.must('PATCH', `/issues/${i.number}`, { state: 'closed', state_reason: 'completed' });
      log(`#${i.number}: closed (CI on ${main} passes again)`);
    }
  }

  const listOpenPulls = async () =>
    (await gh.list('/pulls?state=open'))
      .filter((p) => !p.draft && !p.labels.some((l) => l.name === 'wake') && p.number !== wakePr)
      .sort((a, b) => a.number - b.number);
  const pathsOf = new Map();
  const done = new Set(); // merged in this run (also keeps a dry run from repeating itself)
  const doneHeads = new Map(); // head branch -> head SHA of the PRs merged in this run
  const mergedNow = []; // { number, body, merged_at } of the PRs merged in this run

  // A merge can unblock the PR stacked on it, so sweep again after one (at most 3 passes).
  for (let pass = 0, again = !mainRed; again && pass < 3; pass++) {
    again = false;
    const openPulls = (await listOpenPulls()).filter((p) => !done.has(p.number));

    // A PR whose base branch has been merged into main now targets main. Done
    // on every sweep, so it recovers when the run that merged the lower PR was
    // cut short. The base branch must still be exactly what was merged into
    // main; otherwise retargeting would carry unreviewed commits (a lower PR
    // merged elsewhere, or a branch name used again).
    for (const p of openPulls.filter((x) => x.base.ref !== main)) {
      const mergedShas = new Set(
        (await gh.list(`/pulls?state=closed&head=${owner}:${encodeURIComponent(p.base.ref)}`))
          .filter((x) => x.merged_at && x.base.ref === main)
          .map((x) => x.head.sha),
      );
      if (doneHeads.has(p.base.ref)) mergedShas.add(doneHeads.get(p.base.ref));
      if (mergedShas.size === 0) continue;
      const tip = await gh.get(`/commits/${encodeURIComponent(p.base.ref)}`);
      if (!tip.ok || !mergedShas.has(tip.data.sha)) { log(`#${p.number}: base ${p.base.ref} is not what was merged: left as is`); continue; }
      await gh.must('PATCH', `/pulls/${p.number}`, { base: main });
      log(`#${p.number}: base ${p.base.ref} -> ${main}`);
    }

    for (const summary of openPulls) {
      const n = summary.number;
      if (!pathsOf.has(n)) pathsOf.set(n, touchedPaths(await gh.list(`/pulls/${n}/files`)));
      const paths = pathsOf.get(n);
      let pr = await gh.must('GET', `/pulls/${n}`);
      for (let i = 0; pr.mergeable === null && i < 3; i++) {
        await sleep(4000);
        pr = await gh.must('GET', `/pulls/${n}`);
      }
      const sha = pr.head.sha;
      const labels = pr.labels.map((l) => l.name);
      const comments = await gh.list(`/issues/${n}/comments`);
      // Markers count only in comments of this hand-off (or the owner's account): the repository is public.
      const noted = (kind) => comments.some((c) => trusted(c.user) && (c.body ?? '').includes(marker(kind, sha)));

      // This repository is public. A PR from another repository is never merged here.
      if (pr.head.repo?.full_name !== repo) { log(`#${n}: from another repository: left for the owner`); continue; }
      if (!isApprovalOf(latestReview(comments, owner), sha)) { log(`#${n}: no approval of ${sha.slice(0, 7)}`); continue; }
      // A confirmation of a hold may point to the owner's own word (a comment
      // written without an app), on this PR or on the needs-human Issue.
      const ownerWords = new Map();
      for (const id of confirmationLinks(comments, { owner, repo })) {
        const c = comments.find((x) => x.id === id) ?? (await gh.get(`/issues/comments/${id}`)).data;
        if (c && c.user?.login === owner && appOf(c) === null) ownerWords.set(id, c.created_at);
      }
      if (isHeld(pr, comments, { owner, repo, ownerWords })) { log(`#${n}: held by マージの条件`); continue; }
      if (pr.base.ref !== main) { log(`#${n}: base is ${pr.base.ref} (stacked): wait`); continue; }
      if (pr.mergeable === null) { log(`#${n}: mergeability unknown: next run`); continue; }
      if (pr.mergeable === false) {
        if (noted('conflict')) continue;
        // Label, wake, then the comment with the marker: a sweep cut short
        // before the marker is written does all of it again.
        const role = authorRole(paths);
        await setRole(gh, n, labels, role);
        await wake(role, n);
        await gh.must('POST', `/issues/${n}/comments`, {
          body: `承認済みですが、${main} と競合していてマージできません。\`${role}\` の担当は、${main} を取り込んでください（そのあと、もう一度レビューが要ります）。\n\n${SIGN}\n${marker('conflict', sha)}`,
        });
        log(`#${n}: conflict -> ${role}`);
        continue;
      }

      // CI must have passed on the approved commit. It is looked at on every
      // sweep: a run that failed and passed when it was run again lets the
      // merge go on. The marker only keeps the notice from being repeated.
      const ci = ciState(
        (await gh.list(`/actions/workflows/${ciWorkflow}/runs?head_sha=${sha}`)).filter((r) => r.event !== 'schedule'),
      );
      if (ci.state === 'pending') { log(`#${n}: approved; CI has not finished on ${sha.slice(0, 7)}: next run`); continue; }
      if (ci.state === 'failure') {
        if (noted('ci-failed')) { log(`#${n}: CI failed on ${sha.slice(0, 7)} (already reported)`); continue; }
        const role = authorRole(paths);
        await setRole(gh, n, labels, role);
        await wake(role, n);
        await gh.must('POST', `/issues/${n}/comments`, {
          body: `承認済みですが、最新のコミット \`${sha.slice(0, 7)}\` の CI が通っていません: ${ci.run.html_url}\n\n\`${role}\` の担当は、直してください（直したあと、もう一度レビューが要ります。同じコミットで CI をやり直して通った場合は、そのままマージされます）。手元では \`bash .github/scripts/verify.sh\` で同じ検査ができます。\n\n${SIGN}\n${marker('ci-failed', sha)}`,
        });
        log(`#${n}: CI failed -> ${role}`);
        continue;
      }

      const method = mergeMethod(paths);
      // The sha guard makes the merge fail if the head moved after the checks above.
      const tryMerge = () => gh.must('PUT', `/pulls/${n}/merge`, { merge_method: method, sha }).catch((e) => e);
      let result = await tryMerge();
      // "Base branch was modified" right after another merge, or a head that just
      // moved, is not a reason to wait for the PM: try once more, then leave the
      // PR for the next sweep.
      const transient = (r) => r instanceof Error && /was modified/i.test(r.message);
      if (transient(result)) {
        await sleep(5000);
        result = await tryMerge();
      }
      if (transient(result)) { log(`#${n}: ${result.message.slice(0, 120)}: next run`); continue; }
      if (result instanceof Error) {
        // Tried again on every sweep (the reason may be gone); reported once.
        if (noted('merge-failed')) { log(`#${n}: merge failed again for ${sha.slice(0, 7)} (already reported)`); continue; }
        await setRole(gh, n, labels, 'role:pm');
        await gh.must('POST', `/issues/${n}/comments`, {
          body: `承認済みで CI も通りましたが、仕組みからはマージできませんでした（理由は下のとおり。ワークフローのファイルを変える PR は、仕組みの権限ではマージできません）。PM がマージしてください。\n\n\`${result.message.slice(0, 300)}\`\n\n${SIGN}\n${marker('merge-failed', sha)}`,
        });
        log(`#${n}: merge failed -> role:pm`);
        continue;
      }
      mergedNow.push({ number: n, body: pr.body, merged_at: new Date().toISOString() });
      done.add(n);
      doneHeads.set(pr.head.ref, sha);
      again = true;
      await setRole(gh, n, labels, null);
      await gh.must('POST', `/issues/${n}/comments`, {
        body: `マージしました（${method}。最新のコミット \`${sha.slice(0, 7)}\` へのレビューの APPROVE と、CI の成功を確かめました）。\n\n${SIGN}`,
      });
      log(`#${n}: merged (${method})`);
    }
  }

  // A merge made with this job's token does not start CI on main by itself, so
  // start it: main is checked after every batch of merges. While main is red,
  // CI is started once main has moved past the failed run (the fix was merged
  // by the PM), and its success lets merges go on.
  if (mergedNow.length > 0 || mainRed) {
    const tip = (await gh.must('GET', `/commits/${main}`)).sha;
    const tipRuns = await gh.must('GET', `/actions/workflows/${ciWorkflow}/runs?head_sha=${tip}&branch=${main}&per_page=1`);
    if (tipRuns.workflow_runs.length === 0) {
      await gh.must('POST', `/actions/workflows/${ciWorkflow}/dispatches`, { ref: main });
      log(`CI started on ${main} (${tip.slice(0, 7)})`);
    }
  }

  // Close the Issues that merged PRs complete ("Closes #N" in the PR body).
  // GitHub's own auto-close is not relied on, and a backlog condition
  // "#N が閉じている" waits for it. Recently merged PRs are looked at again on
  // every sweep, so a run cut short after a merge, or a merge made by the PM,
  // is made good. An Issue that someone opened again after the merge is left open.
  const since = Date.now() - CLOSE_LOOKBACK_DAYS * 24 * 60 * 60 * 1000;
  const recentlyMerged = (
    await gh.must('GET', `/pulls?state=closed&base=${main}&sort=updated&direction=desc&per_page=50`)
  ).filter((p) => p.merged_at && Date.parse(p.merged_at) >= since && p.number !== wakePr);
  const completed = new Map(mergedNow.map((p) => [p.number, p]));
  for (const p of recentlyMerged) if (!completed.has(p.number)) completed.set(p.number, p);
  for (const p of [...completed.values()].sort((a, b) => a.number - b.number)) {
    for (const n of closingIssues(p.body)) {
      const r = await gh.get(`/issues/${n}`);
      if (!r.ok || r.data.pull_request || r.data.state !== 'open') continue;
      const reopened = (await gh.list(`/issues/${n}/events`)).some(
        (e) => e.event === 'reopened' && Date.parse(e.created_at) > Date.parse(p.merged_at),
      );
      if (reopened) { log(`#${n}: opened again after #${p.number} was merged: left open`); continue; }
      await setRole(gh, n, r.data.labels.map((l) => l.name), null);
      await gh.must('PATCH', `/issues/${n}`, { state: 'closed', state_reason: 'completed' });
      await gh.must('POST', `/issues/${n}/comments`, {
        body: `PR #${p.number} がマージされたので、閉じました（PR の説明の \`Closes #${n}\`）。\n\n${SIGN}`,
      });
      log(`#${n}: closed (completed by #${p.number})`);
    }
  }

  // Release backlog Issues whose start conditions are met (oldest first).
  let openImplPrs = 0;
  for (const p of (await listOpenPulls()).filter((x) => !done.has(x.number))) {
    if (!pathsOf.has(p.number)) pathsOf.set(p.number, touchedPaths(await gh.list(`/pulls/${p.number}/files`)));
    if (isImplementationPr(pathsOf.get(p.number))) openImplPrs++;
  }
  const backlog = (await gh.list('/issues?state=open&labels=backlog')).filter((i) => !i.pull_request);
  const released = {};
  for (const issue of backlog.sort((a, b) => a.number - b.number)) {
    const n = issue.number;
    // Only Issues written on the owner's account are released (this repository is public).
    if (issue.user?.login !== owner) { log(`#${n}: backlog Issue by ${issue.user?.login}: left for the PM`); continue; }
    const spec = parseBacklog(issue.body);
    if (!spec) { log(`#${n}: backlog header is not machine-checkable: left for the PM`); continue; }
    const labels = issue.labels.map((l) => l.name);
    // Both labels at once: an earlier sweep gave the role its label and was cut
    // short before the wake went out. It is finished now, whatever the caps say.
    const resumed = labels.includes(spec.role);
    if (!resumed && (released[spec.role] ?? 0) >= MAX_RELEASES_PER_ROLE) continue;
    if (!resumed && spec.role === 'role:impl' && openImplPrs >= MAX_OPEN_IMPL_PRS) { log(`#${n}: ${openImplPrs} implementation PRs open: wait`); continue; }
    let ready = true;
    for (const c of spec.conditions) {
      if (c.kind === 'merged') {
        const r = await gh.get(`/pulls/${c.number}`);
        ready = (r.ok && r.data.merged === true) || done.has(c.number);
      } else {
        // Closed as done: not "not planned" or "duplicate", and a PR must have been merged.
        const r = await gh.get(`/issues/${c.number}`);
        ready =
          r.ok &&
          r.data.state === 'closed' &&
          !['not_planned', 'duplicate'].includes(r.data.state_reason) &&
          (!r.data.pull_request || Boolean(r.data.pull_request.merged_at));
      }
      if (!ready) break;
    }
    if (!ready) continue;
    // The role's label first, then the wake, and `backlog` comes off last: until
    // then the Issue is still found here, so a wake that failed is sent again.
    if (!resumed) await gh.must('POST', `/issues/${n}/labels`, { labels: [spec.role] });
    await wake(spec.role, n);
    await setRole(gh, n, [...labels, spec.role], spec.role);
    await gh.must('POST', `/issues/${n}/comments`, {
      body: `着手の条件がそろったので、\`${spec.role}\` の担当に渡しました。\n\n${SIGN}`,
    });
    released[spec.role] = (released[spec.role] ?? 0) + 1;
    log(`#${n}: released -> ${spec.role}${resumed ? ' (taken up again)' : ''}`);
  }
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const { GITHUB_TOKEN, REPO, WAKE_TOKEN, WAKE_PR, CI_WORKFLOW } = process.env;
  if (!GITHUB_TOKEN || !REPO) throw new Error('GITHUB_TOKEN and REPO are required');
  let gh = createClient({ repo: REPO, token: GITHUB_TOKEN, wakeToken: WAKE_TOKEN });
  if (process.env.DRY_RUN === 'true') {
    // Read everything, change nothing: writes are only printed.
    const real = gh;
    gh = {
      ...real,
      must: async (method, path, body) => {
        if (method === 'GET') return real.must(method, path);
        console.log(`[dry-run] ${method} ${path} ${JSON.stringify(body ?? {}).slice(0, 120)}`);
        return { number: 0 };
      },
      call: async (method, path) => {
        if (method === 'GET') return real.call(method, path);
        console.log(`[dry-run] ${method} ${path}`);
        return { ok: true, status: 200, data: {} };
      },
      wake: async (wakePr, line) => console.log(`[dry-run] ${line}`),
    };
  }
  await run({ gh, repo: REPO, wakePr: Number(WAKE_PR), ciWorkflow: CI_WORKFLOW || 'ci.yml' });
}
