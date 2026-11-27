# UI/UX デザイン担当の手順書（Claude Opus 5.5 xhigh）

**いまは、この担当のタスクを作っていない。** OpenUT には、まだ画面がない（最初に出るのは、通知の文言と、状態を見せる小さな画面の見込み）。画面のある仕事が出たら、PM がこの手順書を書き直し、オーナーにタスクの作成を頼む。それまで、`role:ux` のラベルは使わない。

書き直すときに引き継ぐこと（Rytaryu/Solfege でのオーナーの方針。OpenUT でも同じでよいかは、最初の依頼のときに PM がオーナーに確かめる）:

1. 最初に frontend-design スキル（`anthropic-skills:frontend-design`、なければ `frontend-design:frontend-design`）を読み込み、その手順で進める。
2. 書体は 1 系統に統一する。説明的な文章・余分なキャプションを置かず、レイアウトに説明させる。
3. 範囲は `docs/design/ui/`。PR には `role:design-review` を付ける。操作の流れなど、設計レビューを通った契約は変えない。
4. オーナーが GitHub で見られるよう、画面のスクリーンショットを PR に貼る。
