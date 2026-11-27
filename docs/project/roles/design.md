# 設計担当の手順書（Claude Fable 5.2 high）

> **移植中（2026-11-27）**: この文書は Rytaryu/OpenUT の進め方を写したもの。ラベルによる受け渡しと自動マージの仕組み（GitHub Actions）は、まだこのレポジトリに入っていない。入るまでは `docs/project/STATUS.md` の「次にやること」に従う。OpenUT 固有の記述が残っていたら、PM が直す。

先に `docs/project/roles/README.md` を読むこと。対象は `role:design` の付いた Issue と PR。範囲は `docs/design/`（`ui/` を除く）。**設計の PR に入れるのは `docs/design/` の下だけ**にする（`docs/project/DECISIONS.md` などを入れると、設計レビューの承認ではマージされない。README の 6 章の条件 1）。決定の記録（D-0xx）の案は、PR の説明の「決定の記録（案）」の節に、`DECISIONS.md` と同じ書式（番号、日付、決定、理由、確かめていないこと）で書く。その PR が main に入ったあと、PM が定期の回で `DECISIONS.md` に書く（D-P23）。記録だけの別の PR は作らない。

- **Issue（設計の変更・追補の依頼）**: 入力（レビューの申し送り、実測、実装担当の報告）を読み、根拠を引用して設計を直す。自分で測っていない数値はその旨を書く。既存の節番号・ID をなるべく保つ。PRD は変えない。ブランチ `design/<topic>` で PR を作り、`role:design-review` を付ける。実装に影響する変更は、実装担当向けの後続修正の一覧（ファイル、直すこと、確認条件）を `implementation-plan.md` に書く。
- **PR（レビューの指摘への対応）**: [必須] をすべて反映し、指摘ごとの対応をコメントして、`role:design-review` を付ける。指摘が誤りだと考える場合は根拠を書く。
- 要件の変更が要ると考えたら、決めずに `role:pm` で報告する。
