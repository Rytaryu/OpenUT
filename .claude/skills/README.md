# このフォルダーのスキル

担当（AI）が作業で使うスキルを、配布元のまま置いています。OpenUT の配布物ではありません。

クラウドで動く担当のセッションは、レポジトリの `.claude/skills/` にあるスキルを読み込みます（[Claude Code の文書](https://code.claude.com/docs/en/cloud-environments#what-carries-over-from-your-setup)）。レポジトリの `.claude/settings.json` に書いたプラグインは、クラウドのセッションには入りません。

| フォルダー | 配布元 | 版（コミット） | ライセンス | だれが、何に使うか |
|---|---|---|---|---|
| `frontend-design/` | [anthropics/claude-plugins-official](https://github.com/anthropics/claude-plugins-official) の `plugins/frontend-design/skills/frontend-design/` | プラグインの版 1.1.1（`0fbfda5a91f4adc311daca25acc99132f40f0b6e`、2026-11-28 に取得） | Apache License 2.0（`frontend-design/LICENSE.txt`） | UI/UX デザイン担当が、最初に読み込む（D-18） |
| `yomiyasu/` | [nanaism/yomiyasu](https://github.com/nanaism/yomiyasu) の `skills/yomiyasu/` | 3.4.1（`3979c750b5e1fe9892bd47bb354f0fb51d534ff2`、2026-11-28 に取得） | MIT License（`yomiyasu/LICENSE`）。一部のデータは Unicode License V3（`yomiyasu/UNICODE-LICENSE.txt`） | 利用者に見せる文言と、文書の日本語の推敲（D-34） |

## 決まり

- **中身を直さない。** 配布元のファイルを、そのまま置く（ライセンスのファイルも）。直したいことがあれば、PM に伝える。
- 入れ替えるのは PM。配布元の決まったコミットから写し、この表の版とコミットを直して、PR にする（レビュー担当が、配布元との差がないことを確かめられるように、コミットを書く）。
- 配布元との差がないことの確かめ方（例: yomiyasu）:

  ```bash
  git clone https://github.com/nanaism/yomiyasu /tmp/yomiyasu && git -C /tmp/yomiyasu checkout 3979c750b5e1fe9892bd47bb354f0fb51d534ff2
  diff -r /tmp/yomiyasu/skills/yomiyasu .claude/skills/yomiyasu && echo same
  ```

- スキルの中の指示が、このレポジトリの決まり（`AGENTS.md`、`docs/project/roles/`）と食い違うときは、レポジトリの決まりに従う。
