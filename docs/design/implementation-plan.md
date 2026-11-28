# 実装計画

設計担当が書き、実装担当はこの順に PR を出す。最終更新: 2026-11-28（初版。M1。Issue #5）。M2 より先の項目は、その段階の設計で足す。

## 0. 決まり

- 1 項目 = PR 1 本。項目の範囲のファイルの外を直す必要が出たら、PR の説明に理由を書く。範囲を大きく超えるなら、項目を分ける案を `role:pm` で出す。
- **順序の決まり**は、各項目の「順序」の欄。「〜のあと」は、その項目の PR が main に入っていること。「〜のブランチに積んでよい」とあるものは、前の PR がまだ main に入っていなくても、その PR のブランチを base にして出してよい（`OPERATING.md` の 3 章）。
- テストは実装担当が書く（D-18）。受け入れ条件の「テスト」の欄にあるものを、同じ PR に入れる。テストの名前は、何を確かめるかが分かる英語にする。
- すべての PR で `bash .github/scripts/verify.sh` を通す。Linux の CI でも通るよう、Windows だけの部品は `#[cfg(windows)]` と `[target.'cfg(windows)'.dependencies]` で分ける。判定（入出力なし）の部品は、Linux でもビルドとテストができること。
- 設計の節の番号（`m1-watcher.md` の「4.2」など）は、PR の説明とコードのコメントで参照してよい。設計と違う実装にしたら、PR の「未解決・判断を仰ぎたい点」に書く。
- 実機に命令を送るコードは、M1 には入れない（`m1-watcher.md` の 11 章）。
- ログの書式のメモ（Issue #7）が main に入ったら、設計の追補（4.4 の表と 4.8）が出る。それより前に作った M1-3 の語句と、合成のケース（M1-2）は、追補に合わせて直す（後続の修正の一覧。3 章）。

## 1. クレートとワークスペースの構成

`overview.md` の 4 章の構成にする。M1-1 で、いまの最小のクレート（PR #2 の `Cargo.toml`、`src/lib.rs`、`src/main.rs`）を、次の形に変える。

```
Cargo.toml                         ワークスペースと、アプリのパッケージ openut
src/main.rs                        コマンドの振り分け
src/lib.rs                         アプリの部品（tests/ から使う）。VERSION はここに残す
src/cli.rs                         引数の解析（標準ライブラリだけで書く）
src/config.rs                      設定（m1-watcher.md の 9.1）
src/paths.rs                       保存場所（9.3）
src/discover.rs                    ログのフォルダーを探す（9.2）
src/source_fs.rs                   LogSource の実ファイル版（4.1、4.2）
src/watcher.rs                     見張りのループ（時計と眠りを引数で受ける）
src/summary_io.rs                  集計の書き出し（8 章）
src/replay.rs                      expect.toml の読み込みと比較（10.3）
src/notify/mod.rs                  案内の出口のトレイト NoticeSink と振り分け
src/notify/console.rs              コンソール
src/notify/toast.rs                デスクトップ（cfg(windows)）
src/notify/steamvr.rs              SteamVR の中（cfg(windows)）
src/ui/                            トレイと状態の窓（cfg(windows)）
tests/replay.rs                    再生テスト
tests/replay/<ケース>/             再生テストの入力と期待（10.1）
tests/README.md                    要件とテストの対応の表
crates/openut-core/                time, tracker, event, state, rules, notice, text, summary, engine
crates/openut-hublog/              decode, header, record, map, tail, replay
crates/openut-anonymize/           ログの匿名化（L-1。Issue #8）
```

レポジトリの直下の `Cargo.toml` の形:

```toml
[workspace]
members = ["crates/*"]
resolver = "3"

[workspace.package]
version = "0.0.0"
edition = "2024"
rust-version = "1.100"
license = "MIT"
repository = "https://github.com/Rytaryu/OpenUT"
publish = false

[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }

[package]
name = "openut"
description = "Unofficial runtime and tools for VIVE Ultimate Tracker on Windows + SteamVR"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
publish.workspace = true

[lints]
workspace = true

[dependencies]
openut-core = { path = "crates/openut-core" }
openut-hublog = { path = "crates/openut-hublog" }
```

- 各クレートの `Cargo.toml` も `[lints] workspace = true` と `*.workspace = true` を使う。
- `openut-core` と `openut-hublog` は、入出力を持たない（`std::fs`、`std::net`、`std::process`、`SystemTime::now`、`Instant::now`、`std::thread::sleep` を使わない）。時刻は引数で受ける。レビューで見る。
- `openut-core` の依存は、なし（`serde` を、機能 `serde` を付けたときだけの任意の依存にしてよい。設定と集計の型に使う）。`openut-hublog` の依存は `openut-core` だけ。
- 匿名化（Issue #8）は `crates/openut-anonymize/` に置き、行の解読と先頭の解析は `openut-hublog` の `decode` と `header` を使う。コマンドは `openut anonymize`（`src/cli.rs` から呼ぶ）。#8 が M1-1 より先に始まった場合は、#8 の PR で M1-1 のワークスペースの形だけを先に作ってよい（そのとき、M1-1 はその形に合わせる）。

## 2. M1 の項目

### M1-1 ワークスペースと、ログのバイト列から行へ（Issue #9）

- **範囲**: `Cargo.toml`、`Cargo.lock`、`src/lib.rs`、`crates/openut-core/`（`Cargo.toml`、`src/lib.rs`、`src/time.rs`）、`crates/openut-hublog/`（`Cargo.toml`、`src/lib.rs`、`src/decode.rs`、`src/header.rs`）。
- **中身**:
  - 1 章のワークスペース。
  - `openut_core::time`: `LogTime`、`UtcOffset`、`LogDate`、地方時とずれから `LogTime` を作る関数、`LogTime` とずれから地方の日付と時刻を返す関数、`YYYY-MM-DD hh:mm:ss.mmm` の表示（`m1-watcher.md` の 3.1）。
  - `openut_hublog::decode`: 少しずつ受け取って行を返す解読（4.2）。文字コードの決め方、奇数バイトとサロゲートの持ち越し、`\r\n` と `\n`、置き換え文字、64 KiB の切り、最後の行の持ち越しと `finish()`、数（`replaced_chars`、`long_lines`）。
  - `openut_hublog::header`: 行の先頭の解析（4.3）。`ParsedLine`、`Entry`。
- **受け入れ条件とテスト**:
  1. `bash .github/scripts/verify.sh` が通る。3 つのクレートのテストが `cargo test --workspace` で動く。いまの `VERSION` のテストが残っている。
  2. `time`: 2026-11-27 22:53:10.123、ずれ +9 → UTC の 2026-11-27 13:53:10.123 の通し時刻と一致する（期待値はテストの中で別の方法〔例: 日数を手で数えた定数〕で用意する）。うるう日（2028-02-29）、年の境目、負のずれ（-5）、分のあるずれ（+5:30）。1970〜2100 年の毎月 1 日について、日付 → 通し日数 → 日付が元に戻る。
  3. `decode`: (a) UTF-16LE・BOM・CRLF の 3 行が 3 行になる。(b) 同じバイト列を 1 バイトずつ渡しても、(a) と同じ結果になる。(c) U+1F600 のサロゲートの対が、渡す区切りの途中で切れても 1 文字になる。(d) 対にならないサロゲートが U+FFFD になり、次の行も読める。(e) UTF-8（BOM あり・なし）、BOM のない UTF-16LE を見分ける。(f) 終わりのない最後の行は、`finish()` まで出ない。(g) 64 KiB を超える行が切られ、`long_lines` が増える。(h) 固定の種の擬似乱数で作ったバイト列（1 万通り）で `panic` しない（依存を足さず、テストの中の簡単な擬似乱数でよい）。
  4. `header`: (a) `[2026-11-27 22:53:10.123 +9 0x1a2b INFO SomeFunc] [12] Add new tracker: (1)` の各部分が読める（例の値は合成）。(b) 秒の小数が 1〜6 桁。(c) ずれ `+9`、`+09`、`-5`、`+5:30`。(d) スレッドの括弧がない行。(e) 関数名に空白がある行。(f) 空の行、`[` だけ、日付が 2026-13-40 の行、先頭が `[` でない行が `NoHeader`。(g) 擬似乱数の文字列で `panic` しない。
- **順序**: 最初。PR #2 が main に入ってから（Issue #9 の着手の条件）。

### M1-2 再生テストの枠組みと、要件とテストの対応の表（Issue #10）

- **範囲**: `crates/openut-hublog/src/replay.rs`、`src/replay.rs`、`tests/replay.rs`、`tests/replay/`（合成のケース）、`tests/README.md`、`Cargo.toml`（`serde`〔`derive`〕と `toml` を足す）。
- **中身**:
  - `openut_hublog::replay`: 入力のファイル（名前とバイト列）を日付の順に受け取り、解読し、先頭を解析して、結果（いまは行の数と `NoHeader` の数。M1-6 のあとは検出と集計）を返す（10.2）。入出力なし。
  - `src/replay.rs`: `expect.toml` の読み込み（10.3 の形）と、実際の結果との比較。失敗のときの一覧の表示。
  - `tests/replay.rs`: ケースごとに 1 つのテスト関数（`#[test] fn synthetic_r1_second_host_command()` など）。検出を比べるテストは、M1-6 が入るまで `#[ignore = "M1-6 で有効にする"]`。加えて、`tests/replay/` の下のディレクトリのすべてに、対応するテスト関数があることを確かめるテスト（ケースの足し忘れを防ぐ。関数の名前は、ディレクトリ名の `-` を `_` にしたもの）。
  - 合成のケース: `m1-watcher.md` の 10.1 の 3 つ（`synthetic-r1-second-host-command`、`synthetic-normal-start`、`synthetic-utf16`）。シリアル番号などは入れない（入れるなら `TRACKER-A` のような置き換えの形）。
  - `tests/README.md`: 要件とテストの対応の表。行は W-1〜W-9 と、5 章の受け入れの基準の 3 つ。列は「確かめ方（単体テスト／再生テスト／オーナーの実機）」「テストの場所（ファイルとテストの名前。まだなければ、入る項目の番号）」。`m1-watcher.md` の 1 章の表を出発点にする。実機でしか確かめられない項目は、PR の「次の担当へ」に一覧にして PM に渡す。
- **受け入れ条件とテスト**:
  1. `verify.sh` が通る（再生テストが CI で動く）。
  2. 比較の単体テスト: 期待と実際が一致する／期待が見つからない／余分な検出／`tracker` が違う／`after`・`before` の境目（`after` ちょうどは一致、`before` ちょうどは一致しない）／`check_rules` にない規則の検出は失敗にしない／`[summary]` の値の一致と不一致。
  3. 3 つの合成のケースが、解読と先頭の解析を通る（`NoHeader` が 0）。`synthetic-utf16` と、元にしたケースの行の数が同じ。
  4. `tests/README.md` の表が、W-1〜W-9 と受け入れの基準の 3 つのすべてを扱っている。
- **順序**: M1-1 のあと（M1-1 のブランチに積んでよい）。

### M1-3 行の種類と、出来事への変換

- **範囲**: `crates/openut-core/src/{tracker.rs,event.rs}`、`crates/openut-hublog/src/{record.rs,map.rs}`。
- **中身**: `TrackerKey`、`Serial`（3.2）、`Timed`、`Event` と付随の型（3.4）。`record`: 本文から `Classified` を作る（4.4 の表）。`map`: `Entry` と `Classified` から `Timed` を作る。読めなかった行の区別（`other`、`unparsed`）と数（4.7）。
- **受け入れ条件とテスト**:
  1. 4.4 の表の行ごとに、合成の本文 → 期待の `Classified` と `Event` のテスト。番号の 3 つの形（`(N)`、`(index: N)`、`Tracker (N)`）。語句の前後に別の文字があっても判定できる。
  2. 値の対応: `map state: 2` → `Synced`、`map status: 6` → `ReuseOk`、`Pose data - status: 2/3/4` → `Ok/Lost/RecentlyLost`、ほかの値 → `Other(v)`。`is connected to host: 0/1` → `connected`。
  3. 語句はあるが番号や値が読めない（`Remove tracker (x)`、`map state:` で終わる、桁あふれ）→ `unparsed` として数え、出来事を出さない。表にない本文 → `other`。
  4. 擬似乱数の本文で `panic` しない。
- **順序**: M1-1 のあと。

### M1-4 追記の読み取り（ファイルの追跡）

- **範囲**: `crates/openut-hublog/src/tail.rs`、`src/source_fs.rs`。
- **中身**: `LogSource` トレイト（一覧と、位置を指定した読み取り）。`Tailer`: いまのファイルの選び方、追記、0 時の切り替わり、縮んだとき・消えたとき、追いつきの始まりと終わり、切り替わったあとの書き足しの数（4.1、4.2、4.5）。見張りの時刻（4.6 の d と L）を計算する `Watermark`。`source_fs`: `std::fs` による `LogSource`（読み取り専用で開き、読んだら閉じる）。
- **受け入れ条件とテスト**（`Tailer` と `Watermark` は、メモリ上の偽の `LogSource` と偽の時刻で確かめる）:
  1. 何回かに分けて追記されたファイルから、すべての行が順に 1 回ずつ出る。区切りが奇数バイトでも同じ。
  2. 起動時に中身があるファイル: 先頭から読み、終わりに着いたときに「追いつきが終わった」が 1 回出る。
  3. 新しい日付のファイルが現れる: 古いファイルの残りと、終わりのない最後の行が出てから、新しいファイルの先頭に移る。そのあと古いファイルが伸びても読まず、`late_writes` が増える。
  4. ファイルが縮む → 先頭から読み直す。ファイルが消える → 残りで最も新しいものに移る。1 つもない → 「ログが見つからない」。
  5. 名前の判定: `viveTrackerServer_20261127.TXT` は対象、`ViveTrackerServer_2026112.txt`、`other.txt` は対象外。
  6. 1 回に読むのは 4 MiB まで。
  7. `Watermark`: d が直近 5 分の最小値になる（5 分に測ったものがなければ最後の値）。追いつきで読んだ分は d に使わない。見張りの時刻 = max(最後の行の時刻, 今 − d − L)。d がなければ最後の行の時刻。行がまだなければ値がない。
  8. `source_fs`: 一時ディレクトリ（`std::env::temp_dir()` の下に、テストごとに別の名前で作る）に UTF-16LE のファイルを書き、追記し、読める。読み取り専用の属性を付けたファイルも読める。
- **順序**: M1-1 のあと。

### M1-5 状態（トラッカーと PC の Wi-Fi 情報）

- **範囲**: `crates/openut-core/src/state.rs`。
- **中身**: `m1-watcher.md` の 3.5 の型と、5 章の遷移。
- **受け入れ条件とテスト**:
  1. 5.1 の表の行ごとに 1 つ以上のテスト（前の状態 → 出来事 → 次の状態とほかに変えること）。
  2. 5.2 の表の行ごとに 1 つ以上のテスト。2 度目の `HostAssigned` で `Held` から `Missing` になる。
  3. `TrackerAdded` なしで出てきた番号が、推し量った状態と `inferred = true` で足される。
  4. 番号のない出来事: 当てはまるトラッカーが 1 台なら、その台に付く。2 台なら状態が変わらず、数が増える。
  5. 同じ状態への遷移で `since` が変わらない。順序の合わない出来事が数えられる。
  6. 1 台だけ、5 台の場合も、同じ遷移になる（台数を決めていない。W-9）。
- **順序**: M1-3 のあと。

### M1-6 検出の規則とエンジン

- **範囲**: `crates/openut-core/src/{rules.rs（または rules/ の下）,engine.rs}`、`crates/openut-hublog/src/replay.rs`（エンジンをつなぐ）、`tests/replay.rs`（合成のケースの `#[ignore]` を外す）、`tests/replay/*/expect.toml`。
- **中身**: R1〜R5（6.2）、エピソード、`Detection`（3.6）、`EngineConfig`（6 章のしきい値の既定値と上書き）、`Engine`（3.7）。この項目では `Notice` を作らない（M1-7）。
- **受け入れ条件とテスト**:
  1. 規則ごとに: 条件を満たした時刻ちょうどに `Raised` が出る。しきい値のすぐ手前（回数が 1 少ない、時間が 1 ms 足りない）では出ない。`Escalated`（R1）、`Resolved`、`Ended` が 6.2 の条件で出る。
  2. R1: エピソードのあいだに `HostAssigned` が来ても、2 回目の `Raised` が出ない。`Ended` のあとの新しい条件で、新しいエピソードになる。`Held` のあいだは、断られた問い合わせが何回あっても出ない。案内するトラッカーが、最後に `HostAssigned` された番号になる。
  3. R4: 直前 12 秒の命令、つながっていた時間が 30 秒未満、前後 2 秒のほかの切断、前後 5 秒の `ServerStarted` のそれぞれで出ない。`Raised` が切断から 5 秒あとに出る。
  4. R3 と R5 が、`tracked_since_connect` で分かれる（同じ喪失で両方は出ない）。R3 のホストの始まりが `HostAssigned` の時刻。
  5. `EngineConfig` で、しきい値を変えると結果が変わる。`rules.r5.enabled = false` で R5 が出ない。
  6. エンジン: 期限が近い順に判定される。時刻が戻る出来事が、いまの時刻として扱われ、数えられる。
  7. 再生テスト: `synthetic-r1-second-host-command` で R1 がちょうど 1 回、`synthetic-normal-start` で検出なし、`synthetic-utf16` で元のケースと同じ結果。
- **順序**: M1-2 と M1-5 のあと。

### M1-7 案内の配信の決まりと文言

- **範囲**: `crates/openut-core/src/{notice.rs,text.rs}`、`crates/openut-core/src/engine.rs`（`Notice` を出力に足す）。
- **中身**: 7.1 の配信の決まり（push の表、同じ案内を繰り返さない決まり、間隔とまとめ、追いつき、出す前に終わったものを捨てる）と、7.1 の文言の表。トラッカーの名前の決め方（3.2。名前の表は引数で受ける）。
- **受け入れ条件とテスト**:
  1. 7.1 の push の表の、規則と段階の組ごとに、`push` の値が表のとおり。
  2. R2 の `Ended` から 5 分以内の同じトラッカーの `Raised` は push しない。5 分ちょうどのあとは push する。R4 は push から 5 分以内の 2 回目を push せず、欄の案内に回数が付く。R1 は 3 分おきの 3 つのエピソードを 3 回とも push する。
  3. 10 秒以内に来た 3 つの push が、優先度の最も高い 1 つと「ほか 2 件」になる。
  4. 追いつきのあいだは push しない。追いつきが終わった時点で続いているエピソードだけが push される。
  5. 出す前に `Resolved` になった push は出ない。
  6. 文言が 7.1 の表のとおり（名前あり・なし〔`トラッカー{N}`〕、ホストが分からないとき〔「ホストのトラッカー」〕、`r5.wait_s` を変えたときの「1 分」の部分）。
- **順序**: M1-6 のあと。

### M1-8 日ごとの集計

- **範囲**: `crates/openut-core/src/summary.rs`、`crates/openut-core/src/engine.rs`（集計をつなぐ）、`crates/openut-core/Cargo.toml`（`serde` を機能 `serde` の任意の依存に、`serde_json` をテストだけの依存に）、`tests/replay/*/expect.toml`（`[summary]`）。
- **中身**: 8 章の値と、日の区切り。JSON の形は、`serde` の機能を付けたときに 8 章の形で書き出せるようにする（書き出しのファイルの I/O は M1-9）。
- **受け入れ条件とテスト**:
  1. 合成の出来事の列で、`connected_s`、`in_use_s`、`tracking_s`、`tracking_ratio`、`first_pose_s`、`losses`、`long_losses`、`host_starts`、`faults`、`detections` が手で計算した値になる。
  2. 0 時をまたぐ状態が、新しいファイルの最初の出来事の時刻で 2 つの日に分かれる。前の日が `final = true` になる。
  3. `in_use_s` が 0 のとき `tracking_ratio` が `null`。
  4. 4.7 の数が `source` に入る。
  5. JSON に書き出したものが 8 章の形（キーの名前と入れ子）になる（文字列で比べるテスト）。
  6. 再生テスト: 合成のケースの `expect.toml` に `[summary]` を書き、一致する。
- **順序**: M1-6 のあと（M1-7 と並べてよい）。

### M1-9 設定、ログのフォルダーを探す、見張りのループ、コマンド

- **範囲**: `src/{main.rs,cli.rs,config.rs,paths.rs,discover.rs,watcher.rs,summary_io.rs}`、`src/notify/{mod.rs,console.rs}`、`Cargo.toml`（`serde_json` を足す）。
- **中身**:
  - 設定（9.1）の読み込みと書き直し、保存場所（9.3）、フォルダーを探す（9.2。既知の候補はまだ空でよい）。
  - 見張りのループ（2 章、4.6）: `LogSource` と時計と眠りを引数で受ける形にして、テストでは偽物を渡す。500 ms ごとの追記の読み取り、`advance_to(見張りの時刻)`、出力の振り分け、60 秒ごとの集計の書き出し、見張り役の状態（7.4 の表）。
  - `NoticeSink` トレイトと、コンソールの出口（`push` の案内を 1 行で出す）。
  - コマンド: `openut watch [--log-dir D] [--config F] [--data-dir D]`（画面なしの見張り。Linux でも動く）、`openut replay <ケースのディレクトリかファイル>... [--expect F] [--set 名前=値]... [--format text|toml]`（不一致なら終了コード 1）、`openut summarize <ファイル>... [--out D]`、`openut --version`。引数がないときは、Windows では M1-11 の常駐（それまでは使い方を表示）、ほかでは使い方を表示。
- **受け入れ条件とテスト**:
  1. 設定: ファイルがない → 既定値。9.1 の例が読める。形が違う → 既定値と理由（止まらない）。名前の変更を書き直して、読み直すと同じ値。
  2. フォルダーを探す: `log_dir` が優先される。`ViveTrackerServer_*.txt` がないフォルダーは使わない。
  3. 見張りのループ（偽の `LogSource` と時計）: 伸びていく合成のログで、R1 の案内がコンソールの出口に 1 回だけ出る。追いつきのあいだに終わった検出は出ない。ログが静かなあいだも、見張りの時刻で R2 の期限が判定される（L を待ってから）。
  4. 集計の書き出し: 一時ディレクトリに `YYYY-MM-DD.json` ができ、書き出しの途中のファイルが残らない（一時ファイルの名前を変える形）。
  5. `openut replay`: 合成のケースで終了コード 0、期待を 1 つ消した `expect.toml` で終了コード 1。`--set r1.count=1000` で R1 が出なくなる。`--format toml` の出力を `expect.toml` として読み直せる。
  6. 性能の目安（12 章）: 合成の大きなログ（100 万行。テストの中で作る）を `openut replay` で流す時間と、ピークのメモリを PR の説明に書く（CI のテストにはしない）。
- **順序**: M1-4、M1-7、M1-8 のあと。**ここで、画面なしで M1 が使える**（PM に報告し、オーナーに試してもらうかを PM が決める）。

### M1-10 デスクトップの通知

- **範囲**: `src/notify/toast.rs`、`Cargo.toml`（`tauri-winrt-notification` を Windows だけの依存に）。
- **中身**: 7.3。トーストの中身（見出しと 1 行）を作る部分は、入出力なしの関数にする。送り主の ID は、確かめられるまでは `Toast::POWERSHELL_APP_ID`。自前の ID の登録の方法を公式の文書で確かめられたら、その方法を PR に書いて入れる（確かめられなければ、入れずに `role:pm` で報告）。設定の `notify.desktop = false` で出さない。
- **受け入れ条件とテスト**:
  1. Windows の CI でビルドとテストが通る。Linux の CI にこの依存が入らない。
  2. 中身を作る関数の単体テスト（push の案内 → 2 行。まとめた案内の「ほか N 件」）。
  3. 実機の確認の手順（オーナーが 10 分以内でできるもの）を PR の「次の担当へ」に書く（PM が `needs-human` にする）。
- **順序**: M1-9 のあと。

### M1-11 トレイと状態の窓

- **範囲**: `src/ui/`、`src/main.rs`（引数なしの常駐）、`Cargo.toml`（`tray-icon`、`eframe`、`rfd` を Windows だけの依存に）。
- **中身**: 7.4 の、トレイ（3 つの見え方、指を置いたときの 1 行、メニュー）、状態の窓（1〜6）、操作の流れ F1〜F5。見た目は、UI/UX デザイン担当の M1 のデザイン（`docs/design/ui/`）が main にあれば従う。なければ、最小の見た目で作り、デザインが入ったら直す（後続の修正の一覧に PM が足す）。
  - 最初に試作で確かめる: `tray-icon` と `eframe` を同じイベントループで動かせるか、窓を閉じたときに隠すだけにできるか、日本語の書体を Windows の書体から読み込めるか。確かめられなければ、PR を出す前に `role:pm` で報告する（設計の見直しが要る）。
  - 画面に出す中身は、入出力なしの「表示用の写し」を作る関数にする（`Snapshot`・続いている案内・集計 → 行と文字列）。
- **受け入れ条件とテスト**:
  1. Windows の CI でビルドとテストが通る。
  2. 表示用の写しの単体テスト: トレイの見え方と 1 行（7.4 の表の状態ごと）、トラッカーの行（状態の 6 つの名前、ホストの印、経過時間）、PC の Wi-Fi 情報の 3 つの表示、集計の値。
  3. 名前の変更が設定に保存され、案内の文言に使われる（M1-9 の設定の関数を通すテスト）。
  4. 実機の確認の手順を PR の「次の担当へ」に書く。
- **順序**: M1-9 のあと（M1-10 と並べてよい）。

### M1-12 SteamVR の中の通知

- **範囲**: `src/notify/steamvr.rs`（と文字の描画のモジュール）、`Cargo.toml`。
- **中身**: 7.2。最初に、`openvr` クレート（`openvr_sys` の CMake と bindgen）が Windows の CI でビルドできるかを確かめ、だめなら `libloading` の代案にする（どちらにしたかと理由を PR の説明に書く）。`VRApplication_Background` でオーバーレイを作れるかも、試作で確かめる（オーナーに頼む手順に入れてよい）。`unsafe` は、OpenVR の初期化（と代案なら関数表の呼び出し）の場所だけ許し、理由を PR に書く。
  - 文字の描画（案内 → RGBA の画像）は、入出力なしの関数にする（書体のバイト列を引数で受ける）。
- **受け入れ条件とテスト**:
  1. Windows の CI でビルドとテストが通る。Linux の CI にこの依存が入らない。
  2. 文字の描画の単体テスト: 決まった大きさの画像になる、長い 1 行が折り返される、書体がないときにエラーを返す（`panic` しない）。テストの書体は、レポジトリに入れずに済む方法（例: 書体がなければテストを飛ばす。飛ばしたことを出す）にする。
  3. SteamVR につながらないとき、10 秒ごとに試し直し、ほかの出口（デスクトップ、状態の窓）は止まらない（つなぎ方の部分をトレイトにして、偽物で確かめる）。
  4. 実機の確認の手順（SteamVR を動かした状態で、合成のログを置いたフォルダーを `--log-dir` で指して、案内が VR の中に出るか。10 分以内）を PR の「次の担当へ」に書く。性能の確認（`m1-watcher.md` の 12 章）も、同じ回に頼む。
- **順序**: M1-9 のあと（M1-10、M1-11 と並べてよい）。

### M1-13 受け入れの基準のテスト（実物のログ）

- **範囲**: `tests/replay/hub-2026-11-22-27/expect.toml`、`tests/replay.rs`（テスト関数を足す）。ログそのものは PM が入れる（L-2）。
- **中身**: 10.4。しきい値を 6.1 の決め方で決め、R1 がちょうど 4 回、受け入れの基準の時刻に出ることを確かめる。決めた値と、ほかの規則の検出の数を PR の説明に書く（設計の表を直すかは PM が決める）。
- **受け入れ条件とテスト**: 再生テストで R1 がちょうど 4 回（2026-11-26 01:43、2026-11-27 22:53・22:56・22:59）。
- **順序**: M1-6 のあと、かつ PM が匿名化した実物のログを `tests/replay/hub-2026-11-22-27/` に入れたあと（PM が依頼を出す）。

## 3. 後続の修正の一覧

設計の変更・追補に伴って、実装担当が直すもの。設計担当が足し、PM が Issue にする。

| 番号 | きっかけ | ファイル | 直すこと | 確認の条件 |
|---|---|---|---|---|
| （なし） | | | | |

ログの書式のメモ（#7）のあとの追補で、M1-3 の語句（`crates/openut-hublog/src/record.rs`）と、合成のケース（`tests/replay/synthetic-*`）の行を直す項目が、ここに足される見込み。
