# 起動のしくみ（各担当のタスクの一覧）

進め方は `docs/project/roles/README.md`。Rytaryu/Solfege と同じ形にする。

**2026-11-27 時点では、どのタスクもまだ設定されていない。** 設定したら、この表を埋める。

## Claude 側（claude.ai の Code のスケジュール済みタスク）

| タスク | 起動のきっかけ | モデル・思考量 | 手順書 | 状態 |
|---|---|---|---|---|
| OpenUT PM | 1 日 1 回 | Fable 5.2・高 | `roles/pm.md` | 未設定 |
| OpenUT 設計 | Issue・PR に `role:design` が付いた時 | Fable 5.2・高 | `roles/design.md` | 未設定 |
| OpenUT 実装レビュー | PR に `role:impl-review` が付いた時 | Opus 5.5・超高 | `roles/impl-review.md` | 未設定 |
| OpenUT UI/UX デザイン | PR に `role:ux` が付いた時 | Opus 5.5・超高 | `roles/ux.md` | 未設定（画面のある作業が出るまで不要） |

## ChatGPT 側（ChatGPT の「スケジュール」のイベント起動タスク）

| タスク | 起動のきっかけ | モデル・思考量 | 手順書 | 状態 |
|---|---|---|---|---|
| OpenUT 実装 | 起動用の PR への `WAKE role:impl` | GPT-6.2 Sol・ウルトラ | `roles/impl.md` | 未設定 |
| OpenUT テスター | 起動用の PR への `WAKE role:test` | Solfege と同じ設定 | `roles/test.md` | 未設定 |
| OpenUT 設計レビュー | 起動用の PR への `WAKE role:design-review` | GPT-6.2 Astra・マックス | `roles/design-review.md` | 未設定 |

## GitHub Actions（AI を使わない受け渡し）

未移植。Solfege の `.github/`（handoff、handoff-tick、owner-reply、ci、`scripts/verify.sh`）を、Python のプロジェクトに合わせて移す。`docs/project/STATUS.md` の「次にやること」の 1 番。
