# いまの状況（PM が更新する）

最終更新: 2026-11-27

## 状況

- レポジトリを作り、要件・設計の初版・調査の記録・進め方の文書を置いた。
- 受け渡しの仕組み（GitHub Actions）、ラベル、各担当のタスクは、まだない。
- 動くプログラムはまだない。

## 次にやること

1. **受け渡しの仕組みを Solfege から移す（PM）。** `.github/workflows/`（handoff、handoff-tick、owner-reply、ci）、`.github/actions/handoff/`、`.github/scripts/`（`handoff.mjs`、`owner-reply.sh`、`verify.sh` とそのテスト）。変える所: `verify.sh` を Python 用（lint、型、pytest）にする。`handoff.mjs` の「E2E が要る経路」の規則をなくす。起動用の PR の番号を合わせる。ラベル（`role:*`、`needs-human`、`backlog`、`wake`）を作る。
2. **`docs/project/` の文書の校正（PM）。** Solfege から写した文書に、このプロジェクトに合わない記述が残っていないか読む。冒頭の「移植中」の注記は、1 が終わったら消す。
3. **段階1（見張り役）の設計（設計担当）。** `docs/design/overview.md` を、実装できる粒度の設計にする。ログの書式、状態の遷移、検出の規則、通知の出し方、再生テストの形。
4. **再生テスト用のログを用意する（オーナーと PM）。** 2026-11-22〜27 のログから、シリアル番号と無線のアドレスを置き換えたものを作る。置き換えの道具を先に作る。
5. **設計レビュー → 実装 → 実装レビュー → テスト**（1 と 3 のあと）。

## オーナーに頼むこと

- GitHub の Secrets に `WAKE_TOKEN` を入れる（Solfege と同じ用途）。
- Claude 側と ChatGPT 側の各担当のタスクを作る（`docs/project/roles/claude-runner.md` の表）。
- トラッカー内のログの収集（`docs/research/findings-2026-11-27.md` の「次に確かめること」）。

## 止まっているもの

- ChatGPT 側の担当（設計レビュー、実装、テスト）は、利用枠の都合で 2026-11-28 の昼以降に動く。
