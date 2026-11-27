# 実装担当の手順書（GPT-6.2 Sol／ウルトラ）

先に `docs/project/roles/README.md` を読むこと。対象は `role:impl` の付いた Issue と PR。それがなくなったら、README の 7 章の手順で `backlog` から取る（`backlog` も空なら、設計の実装計画 `docs/design/implementation-plan.md` から 1 件 Issue にしてよい。README の 7 章）。

## Issue（新しい実装の依頼）のとき

1. Issue の本文を読み、指定された設計（main の `docs/design/`）と範囲のファイルを確かめる。
2. ブランチを切る。土台の PR が main にマージ済みなら main から、まだなら Issue が指定するブランチの最新から。
3. 実装し、単体テストを書く。PR を出す前に、`python -m pip install -e ".[dev]"` のあと `bash .github/scripts/verify.sh`（ruff、mypy、pytest）を通す。同じ検査が、PR ごとに CI で動く（Linux と Windows、Python 3.11 と 3.13）。CI が落ちた PR は、承認のあとでも仕組みがマージせずに戻すので、PR を出したあと、CI の結果を確かめる（`gh api "repos/Rytaryu/OpenUT/actions/runs?head_sha=<SHA>"`）。
4. PR を作る（説明は OPERATING.md 4 章の書式。**その PR で Issue の受け入れ条件がすべて満たされるなら `Closes #<Issue>`、一部だけなら `Refs #<Issue>`** を入れる。`Closes` の Issue は、マージで自動で閉じ、次の仕事の着手の条件になる。積み重ねる場合は base を前の PR のブランチにする）。
5. Issue に、PR の番号と、満たせなかった受け入れ条件をコメントする。
6. 受け渡し: PR に `role:impl-review`。

## オーナーの返事が届いたとき（README の 8 章）

実機（トラッカー）での確認の結果などのオーナーの返事（アプリを通していないコメント）を読み、直すべき点を Issue にして（1 つの Issue に 1 つの目的）着手する。設計の変更が要る点は `role:pm` に回す。

## PR（レビューの指摘への対応）のとき

1. 最新の `REVIEW: REQUEST_CHANGES` と、そのあとの PM・設計担当のコメントを読む。
2. [必須] をすべて直す。[推奨] は対応するか、見送る理由を書く。指摘が誤りだと考える場合は根拠を書く。設計が原因なら、直さずに `role:pm` で報告する。
3. base のブランチや main が進んでいれば取り込む（強制プッシュはしない）。この PR の上に積んだ PR があれば、同じ修正を取り込む。
4. 指摘ごとの対応をコメントする。
5. 受け渡し: PR に `role:impl-review`。

## 実機に関わる変更

- ドングルやトラッカーに命令を送るコードは、既定で送らない設定にし、送る命令を PR の説明に列挙する（`AGENTS.md`）。
- 送ってよい命令は、設計（`docs/design/`）の一覧にあるものだけ。一覧にない命令、再起動させる命令、書き込みの命令は入れない。立て直しのための命令（D-13）も、設計の一覧に入り、オーナーが実機で確かめたものだけを、既定で無効のまま入れる。
- 通信の手順は、他人のコード（`pyvut`、`vive_ultimate_tracker_re`。どちらもライセンスの表示がない）を写さずに自分で書き、参考にした場所を PR の説明に書く。
- テスト用のログとデータには、実物のシリアル番号、無線のアドレス、Wi-Fi の名前とパスワード、PC の名前、利用者のフォルダー名を入れない。

## 設計との食い違い

設計と違う実装にする場合は、理由を PR の「未解決・判断を仰ぎたい点」に書く。設計の誤りに気づいたら、`role:pm` の Issue を作って報告する。
