# 実装計画

設計担当が書き、実装担当はこの順に PR を出す。最終更新: 2026-11-28（画面と通知で見せるものの追補。Issue #27。PRD の W-10〜W-12）。M2 より先の項目は、その段階の設計で足す。

## 0. 決まり

- 1 項目 = PR 1 本。項目の範囲のファイルの外を直す必要が出たら、PR の説明に理由を書く。範囲を大きく超えるなら、項目を分ける案を `role:pm` で出す。
- **順序の決まり**は、各項目の「順序」の欄。「〜のあと」は、その項目の PR が main に入っていること。「〜のブランチに積んでよい」とあるものは、前の PR がまだ main に入っていなくても、その PR のブランチを base にして出してよい（`OPERATING.md` の 3 章）。
- テストは実装担当が書く（D-18）。受け入れ条件の「テスト」の欄にあるものを、同じ PR に入れる。テストの名前は、何を確かめるかが分かる英語にする。
- すべての PR で `bash .github/scripts/verify.sh` を通す。Linux の CI でも通るよう、Windows だけの部品は `#[cfg(windows)]` と `[target.'cfg(windows)'.dependencies]` で分ける。判定（入出力なし）の部品は、Linux でもビルドとテストができること。
- 設計の節の番号（`m1-watcher.md` の「4.2」など）は、PR の説明とコードのコメントで参照してよい。設計と違う実装にしたら、PR の「未解決・判断を仰ぎたい点」に書く。
- 実機に命令を送るコードは、M1 には入れない（`m1-watcher.md` の 11 章）。
- ログの書式のメモ（`docs/research/hub-log-format.md`。以下「メモ」）と見本（`docs/research/hub-log-samples.md`）に合わせた追補（Issue #18）で、`m1-watcher.md` の 4〜10 章を直した。すでに main に入った M1-1・M1-2 を直す項目は、3 章の F-1〜F-3。M1-3 より先の項目は、この版の設計で作る。
- 画面と通知で見せるものの追補（Issue #27。PRD の W-10〜W-12）で、`m1-watcher.md` の 3.2〜3.7（呼び名、出来事、状態の写し、`Detection` の `serial`）、4.4（読む行）、5.1、5.4〜5.6（台帳、ドングル、VIVE Hub のサーバー。足した）、6.2 の R1、7 章（文言、表示の写し、手引き、仕組みの側の不具合）、8 章（`hub_number`）、9.1（ニックネームをやめた）を直した。M1-3 より先の項目は、まだ main に入っていないので、この版の設計で作る（各項目に書き足した）。main に入った部分の直しは、3 章の F-4。PR #37 のレビュー（1 回目）の指摘で、全体の見え方を 3 つにし（灰色）、VIVE Hub が止まったときの見せ方、`pick_folder`、`Detection`・`Notice` の `conn`、`NoticeText` を足し、VIVE Hub の番号をいまの接続の値に、並べ方をシリアル番号の順にした（M1-5〜M1-7、M1-11 に書き足した）。
- テストの入力には、見本の行（個人の情報は置き換え済み）をそのまま使ってよい。実物のログの断片は入れない（匿名化の道具〔L-1〕を通したものだけ。PRD の L-2）。

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
src/hub_process.rs                 VIVE Hub のプロセスがあるか（5.6。cfg(windows) で取得、判定は入出力なし）
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
crates/openut-core/                time, tracker, event, state, rules, notice, text, display, summary, engine
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
- **順序**: 最初。PR #2 が main に入ってから（Issue #9 の着手の条件）。main に入った（PR #15）。追補による直しは 3 章の F-1・F-2。

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
- **順序**: M1-1 のあと（M1-1 のブランチに積んでよい）。main に入った（PR #16）。追補による直しは 3 章の F-3。

### M1-3 行の種類と、出来事への変換

- **範囲**: `crates/openut-core/src/{tracker.rs,event.rs}`、`crates/openut-hublog/src/{record.rs,map.rs}`。
- **中身**: `TrackerKey`、`Serial`、`HubNumber`、`TrackerLabel`（`m1-watcher.md` の 3.2。呼び名を作る関数は M1-7）、`Timed`、`Event` と付随の型（3.4。`Status`〔`battery` を含む〕、`Battery`、`PowerOffNotice`、`ServerStopped`、`DongleAdded`、`DongleRemoved`、`DongleReport`、`usb`、`SerialKnown` の `hub_number` と `firmware`、番号なしの `PcCommand` を含む）。`record`: `Entry` から種類と値を読む（4.4 の 3 つの表。サーバーの表、命令の表、ドライバーの表）。本文の先頭の形で比べ、`Camera frame rate:` だけは関数名も見る。状態の行（`DeviceStatusToVHServer`）の `key: value` の読み取り（`bIsConnectedOnUSB` は任意のキーで、`Status` の `usb`。`percentage` と `charging` も任意のキーで、`Status` の `battery`。4.4 の注）。`GetDeviceInfoAndResponse` の行の `model name` と `fw version`（4.4 の注。読めなくても `SerialKnown` は出す）と、シリアル番号の文字の決まり。`Dongle is added.`（`DongleAdded`）と `[ServiceStatusToVHServer]` の行の `dongle connected`（`DongleReport`）。`map`: `Entry` と種類から `Timed` を作る。ファイルの種類（サーバーかドライバーか）を引数で受け、ドライバーのファイルでは予告の行だけを出来事にする。読めなかった行の区別（`other`、`unparsed`、`driver.lines`）と数（4.7）。
- **受け入れ条件とテスト**:
  1. 4.4 のサーバーの表の行ごとに、**見本（`hub-log-samples.md`）の行**を入力にして、期待の種類と `Event` になるテスト（見本にない形〔`switch to CLIENT mode.`、`Query wifi from tracker N` など〕は、見本の 4 章の「形だけの見本」の行を使う）。命令の表の行ごとに、`PcCommand` の `kind` と番号（`Pair tracker, isPair: 0, device index:-2` → 番号なし）。ドライバーの表（見本の 5 章の予告の行 → `PowerOffNotice(2)`。`Wait for HID data got timeout.\n` → 出来事なし、`driver.lines` だけ）。
  2. 番号のない 2 つの行（`Got host wifi info.`、`Wifi info is not yet available.`）が、`None` の出来事になる。`Remove tracker (2), connection type: 1, power state:11 `（末尾の空白つき）と `connection type: 2` の `usb`。`GetDeviceInfoAndResponse` の行から `SerialKnown(2, "00XX000000001")`。`sn: ()` は出来事にしない。`sn: (` のない `Response - cmd type:` の行は `other`。
  3. 状態の行: 見本の 3 つの状態の行から、`connected`、`host`、`map_ready`（`map state: 2` か `map status: 6`）、`usb`（`bIsConnectedOnUSB: 0` → `Some(false)`。`1` に書き換えた行 → `Some(true)`）。負の値（`map ID: -1`、`map status: -1`）が読める。`device index` などが欠けた行は `unparsed`。`bIsConnectedOnUSB` だけが欠けた行は `unparsed` にならず、`usb` が `None`。
  4. 値の対応: 姿勢の行の `status: 2/3/4` → `Ok/Lost/RecentlyLost`、0 と 47 などの値 → `Other(v)`。`is connected to host: 0/1` → `connected`、2 → `unparsed`。
  5. 間違えやすい行が種類にならない: `(SteamVR: -1)(0)(tracking status: 0) Position (-nan, -nan, -nan) includes NAN.` → `other`。`OETServer::ParseConfig` の `Camera frame rate: 50` → `other`。`This OET has latest firmware` → `other`。`Wake up dongle.` と `Wake up dongle: (2).` の区別。
  6. 形の先頭は合うが番号や値が読めない（`Remove tracker (x), ...`、桁あふれ、`Pose data - status: ` で終わる）→ `unparsed` として数え、出来事を出さない。表にない本文 → `other`。
  7. 擬似乱数の本文で `panic` しない。
  8. **画面に見せる項目**（W-11、W-12。#27）: (a) 見本の 4 章の `GetDeviceInfoAndResponse` の行 → `SerialKnown(2, "00XX000000001", Some(1), Some("1.0.999.178"))`。`model name: (VIVE Ultimate Tracker)`（数なし）、`(Other Device 1)`、`(VIVE Ultimate Tracker 0)`、`(VIVE Ultimate Tracker 100)`、`model name` のない行 → `hub_number` が `None` で、`SerialKnown` は出る（`unparsed` に数えない）。`fw version: ()`、33 文字の版、空白を含む版、キーのない行 → `firmware` が `None` で、`SerialKnown` は出る。`sn: (00XX 0001)`（空白）と、制御文字を含むシリアル番号 → `unparsed`。`sn: (TRACKER-A)` は受け付ける。(b) 見本の 4 章の状態の行（`connected: 1, ..., charging: 0, percentage: 98`）→ `battery = Some(Battery { percent: 98, charging: Some(false) })`。見本の 1 章の `connected: 0` の行（`percentage: 0`）→ `None`。`connected: 1` で `percentage: 0`、`101`、`-1`、`x`、キーがない → `None` で、行は `unparsed` にならない（ほかの値は今までどおり読める）。`charging: 2`・キーがない → `charging` が `None`。(c) 見本の 1 章の `Dongle is added. version: (0909.0000.0000.0917)` → `DongleAdded`。見本の 2 章の `[ServiceStatusToVHServer] dongle connected: 1, ...` → `DongleReport { connected: true }`、`dongle connected: 0` → `false`、`dongle connected: 2`・キーがない → `unparsed`。`dongle count: 0, tracker count: 0` → `other`。(d) 見本の 2 章の `Peripheral data changed - is charging: 0, battery percent: 74, ...` → `other`（番号がないため。4.4）。
- **順序**: M1-1 のあと、かつ、この追補（Issue #18 の PR）が main に入ったあと。3 章の F-1・F-2 のあと（同じ PR に入れてもよい）。#27 の追補（W-10〜W-12）の PR が先に main に入っていれば、受け入れ条件の 8 も同じ PR に入れる。M1-3 の PR が先に main に入ったときは、8 を 3 章の F-5 として後から足す。

### M1-4 追記の読み取り（ファイルの追跡）

- **範囲**: `crates/openut-hublog/src/tail.rs`、`src/source_fs.rs`。
- **中身**: `LogSource` トレイト（一覧と、位置を指定した読み取り）。`Tailer`: ファイルの種類（`ViveTrackerServer_`、`ViveTrackerDriver_`）ごとに 1 つ。いまのファイルの選び方、追記、0 時の切り替わり（日が飛んだかどうか）、世代、作り直しの見分け方（縮んだ、先頭の 4 KiB が変わった、消えた）、起動と再構築のときの読む順（前の日のファイル、いまのファイル）、追いつきの始まりと終わり、切り替わったあとの書き足しの数（4.1、4.2、4.5）。`Tailer` は、バイト列（ファイル名と世代つき）と、「再構築する（理由）」「読み終えた」（4.5。追いつきの終わりそのものは、M1-9 の見張りのループが、持っている記録を渡し終えたときに決める）「次のファイルに移った（日が飛んだか）」「切り替わったあとの書き足し」「ログが見つからない」を順に返す（エンジンを呼ぶのは M1-9 の見張りのループ）。見張りの時刻（4.6 の d と L。サーバーのファイルだけで測る。読み終えた時の d₀ を含む）を計算する `Watermark`。2 つの列で 1 つのずれの補い（`OffsetFill`。F-2。基準が決まる前のドライバーの記録は、そこで持たれる）を通してから、2 つの `Tailer` の記録の列を合わせる部品（4.6 の「2 つのファイルの並べ方」の 1〜3）: ドライバーの記録は読んだらすぐ出し、サーバーの記録はファイルの順に、外から渡される上限 D（エンジンの `driver_needed_through()`）と W_drv で、D より後のものを持つ。W_drv（ドライバーの見張りの時刻。`Watermark` と同じ d と L）の計算と、`driver.late` の判定（読んだ時点で、前の W_drv より前の時刻の記録）。追いつきのあいだの W_drv（4.6 の 3: 読み終えるまではドライバーで最後に読んだ記録の時刻。追記が続きうるいまのファイルの終わりでも、待ちを解かない。待ちでサーバーの記録を持っているあいだは、ドライバーを先に読み、ドライバーがいまのファイルの終わりに着いていればサーバーを読み進める。読み終えたら、d₀ を含む d で 2 の W_drv に戻す）。名前の末尾に `_N` の付いたファイルを見つけたことを返す（4.1。アプリのログに 1 回だけ残すのは M1-9）。どちらかのいまのファイルの作り直しで、両方を読み直す（4.1）。`source_fs`: `std::fs` による `LogSource`（読み取り専用で開き、読んだら閉じる）。
- **受け入れ条件とテスト**（`Tailer` と `Watermark` は、メモリ上の偽の `LogSource` と偽の時刻で確かめる）:
  1. 何回かに分けて追記されたファイルから、すべての行が順に 1 回ずつ出る。区切りが奇数バイトでも同じ。
  2. 起動時に中身があるファイル: 先頭から読み、サーバーとドライバーの両方がいまのファイルの終わりに着いたときに「読み終えた」が 1 回出る。
  3. 新しい日付のファイルが現れる: 古いファイルの残りと、終わりのない最後の行が出てから、新しいファイルの先頭に移る。そのあと古いファイルが伸びても読まず、`late_writes` が増える。
  4. ファイルが縮む → 「再構築する（縮んだ）」が 1 回出て、前の日のファイル（あれば）といまのファイルを、新しい世代として先頭から読み直す（追いつき）。読んだ位置より大きくなったうえで、先頭の 4 KiB が前と違う → 「再構築する（置き換えられた）」で同じ。先頭が同じで伸びただけ → 再構築しない。ファイルが消える → 「再構築する（消えた）」で、残りで最も新しいものを読む。1 つもない → 「ログが見つからない」。そのあとファイルが現れる → 再構築。
  4a. 再構築のあとのバイト列には新しい世代の番号が付き、前の世代の解読の途中の状態（持ち越したバイト、終わりのない行）が新しい世代に混ざらない。
  4b. 起動時: いまのファイルの 1 日前の日付のファイルがあれば、それを先に読み、そのあといまのファイルを読む。2 日前のファイルは読まない。1 日前のファイルがなければ、いまのファイルだけ。
  4c. 0 時の切り替わりで、新しいファイルの日付が次の日なら「次のファイルに移った（日は飛んでいない）」、2 日以上先なら「（日が飛んだ）」が出る。
  5. 名前の判定: `viveTrackerServer_20261127.TXT` と `ViveTrackerDriver_20261127.txt` は対象（種類つき）、`ViveTrackerServer_2026112.txt`、`ViveTrackerDongle_20261127.txt`、`other.txt` は対象外。`ViveTrackerServer_20261127_1.txt` は対象外で、見つけたことが返る。
  5a. ドライバー: サーバーのいまのファイルが 11/28 に移っても、ドライバーのファイルが 11/27 のまま伸び続けていれば、11/27 のドライバーのファイルを読み続け、11/28 のドライバーのファイルが現れたら移る（メモの 2.5。ドライバーの切り替わりは遅い）。ドライバーのファイルがなくても、サーバーのファイルの読み取りは止まらない。ドライバーのファイルの作り直しで、サーバーとドライバーの両方を読み直す。
  5b. 合わせる部品（4.6 の 1〜3）: 上限 D がないとき、サーバーの記録は届いた順にすべて出て、ドライバーの記録は読んだらすぐ出る。上限 D があり W_drv ≤ D のとき、時刻が D 以下のサーバーの記録は出て、D より後のものは出ない。W_drv が D を超えたら（ドライバーの記録が来た、または偽の壁時計が L 進んだ）、持っていた記録が順に出る。ドライバーのファイルがないとき、D があっても待たない。読んだ時点で前の W_drv より前の時刻のドライバーの記録が `driver.late` に数えられる。追いつき（4.6 の 3。レビュー〔2 回目〕の指摘）: 読み終えるまでは、W_drv が最後に読んだドライバーの記録の時刻になる。ドライバーがいまのファイルの終わりに着いても、待ちが解けない（初版の「終わりでは待たない」はやめた）。前の日のドライバーのファイルの終わりでは、次のファイルに移るだけで、待ちは解けない。待ちのあいだ、ドライバーに読み切れない分があればドライバーが先に読まれ、ドライバーがいまのファイルの終わりならサーバーが読み進められて「読み終えた」に着く。読み終えたあとは、偽の壁時計が進むと W_drv が d₀ で進み、期限 D の待ちが、読み終えてから max(0, D − サーバーの最後の記録の時刻) + L を過ぎたところで解ける。D がサーバーの最後の記録より L 以上前なら、読み終えた時に解ける。先頭の読めない行と `#` の行が、同じ列の前の記録に続いて出る。基準のずれが決まる前のドライバーの記録は、サーバーの最初の記録のあとに出る（F-2 の (j)）。
  6. 1 回に読むのは 4 MiB まで。
  7. `Watermark`: d が直近 5 分の最小値になる（5 分に測ったものがなければ最後の値）。追いつきで読んだ分は d に使わない。ただし、読み終えた時に、d₀ = その時の壁時計 − サーバーで最後に読んだ記録の時刻 が 1 つの測定として入り、あとで小さい値を測れば、それに替わる（4.6）。見張りの時刻 = max(最後の行の時刻, 今 − d − L)。d がなければ最後の行の時刻。行がまだなければ値がない。W_drv = max(ドライバーの最後の記録の時刻, 今 − d − L)（d はサーバーで測ったもの。d がなければドライバーの最後の記録の時刻）。
  8. `source_fs`: 一時ディレクトリ（`std::env::temp_dir()` の下に、テストごとに別の名前で作る）に UTF-16LE のファイルを書き、追記し、読める。読み取り専用の属性を付けたファイルも読める。
- **順序**: M1-1 のあと。3 章の F-1・F-2 のあと（記録の切り方が変わるため。2 つの列で 1 つのずれの補いを使うため）。

### M1-5 状態（トラッカーと PC の Wi-Fi 情報）

- **範囲**: `crates/openut-core/src/state.rs`。
- **中身**: `m1-watcher.md` の 3.5 の型（`Link`、`ConnId`、`HostRef`〔`conn` と `serial` を持つ〕、`Identity`、`DongleState`、`HubState`、`Connection`、`DeviceView`〔`conn` と `hub_number` を持つ〕、`TrackerView::hub_number` を含む）と、5 章の遷移（5.1 のトラッカーと接続と電池と VIVE Hub の番号、5.2 の PC の Wi-Fi 情報、5.3 のホストの根拠、5.4 の台帳と画面に並べる台〔`Snapshot::devices` の組み立て〕、5.5 のドングル、5.6 の VIVE Hub のサーバー）。接続の始まりと終わり（つなぎ直しを含む）を、R4（M1-6）と集計（M1-8）が使える形で出す（例: 状態の更新の結果として「N の接続 c が時刻 a で終わった・始まった」を返す）。
- **受け入れ条件とテスト**:
  1. 5.1 の表の行ごとに 1 つ以上のテスト（前の状態 → 出来事 → 次の状態とほかに変えること）。
  2. 5.2 の表の行ごとに 1 つ以上のテスト。2 度目の `HostAssigned` で `Held` から `Missing` になる。5.3 の表の行ごとに 1 つ以上のテスト。
  3. `TrackerAdded` なしで出てきた番号が、推し量った状態と `inferred = true` で足される。
  4. 番号のない出来事（5.1）: `WifiRequestUnanswered` で、候補（接続か Wi-Fi 待ちで、役割が `Host` でない台）が 1 台ならその台が Wi-Fi 待ちになる。2 台・0 台なら状態が変わらず、数も増えない。`HostWifiObtained` で `Held` になり、ホストは変わらない。
  4a. **状態の行から始まる**（5.3 の例。レビューの指摘）: 新しい状態（PC の Wi-Fi 情報は `Unknown`）に、`Status(2, connected, host = true)`、`Status(4, connected, host = false)`、`Status(0, connected, host = false)` を渡すと、ホストが 2（`Report`）、2 の役割が `Host`、Wi-Fi 情報は `Unknown` のまま。続く `WifiRequestUnanswered` で、候補は 4 と 0（2 は入らない）なので状態が変わらない。`ClientLink(0, true)` のあとなら、候補は 4 だけで、4 が Wi-Fi 待ちになる。
  4b. **ホストの強さ**: `HostAssigned(0)` のあとの `Status(2, connected, host = true)` ではホストが 0 のまま。`Status(2, ...)`（報告）のあとの `HostAssigned(0)` でホストが 0（`Command`）になり、2 の役割が `Unknown` になる。`Status(2, connected = false, host = true)` ではホストにならない。`HostWifiQueried(3)` は、ホストがなければ 3（`Report`）になり、命令のホストが 0 なら変わらず、順序の合わない出来事が 1 増える。`ServerStarted`・`ServerStopped` でホストがなくなる。`TrackerRemoved` と `DongleRemoved` では残り、接続の終わったホストになる。
  4c. **ホストの根拠と接続**（5.3。レビュー〔2 回目〕の指摘）: `HostAssigned(2)` のあと、`TrackerRemoved(2)`、`TrackerAdded(2)`（別の台 B）→ ホストの `key` は 2 のままで、`conn` が B の接続と違う（接続の終わったホスト）。B の役割は `Unknown` で、`WifiRequestUnanswered` の候補に入る（候補が B だけなら、B が Wi-Fi 待ちになる）。B の `Status(2, connected, host = true)` で、ホストが B の接続（`Report`）に替わる（接続の終わった命令のホストは、報告で置き換わる）。接続の続いている命令のホストは、報告で置き換わらない（4b のまま）。`HostAssigned(2)` のあとの `SerialKnown(2, S)` で、ホストの `serial` が S になる。B の接続になってからの `SerialKnown(2, S2)` では、ホストの `serial` は S のまま。未接続の番号への `HostAssigned` で、ホストがなくなる。
  5. 同じ状態への遷移で `since` が変わらない。順序の合わない出来事が数えられる。
  6. 1 台だけ、5 台の場合も、同じ遷移になる（台数を決めていない。W-9）。
  7. **重なり**（5.1）: `TrackerRemoved` が 3 ms おいて 2 回 → 1 回目だけで未接続になり、2 回目は数えない。`TrackerAdded` が 1 秒以内に 2 回 → 2 回目は何もしない（ホストの役割、`conn` などが変わらない）。1 秒と 1 ms あとの 2 回目 → つなぎ直しとして接続になり、順序の合わない出来事が 1 増える。**つなぎ直しで接続を区切る**（レビュー〔2 回目〕の指摘）: そのとき、前の接続がその `TrackerAdded` の時刻 a で終わり、同じ a に新しい `conn` の接続が始まったことが返る。シリアル番号と役割がなくなり、ホストだった台なら、接続の終わったホストになる。推し量って足した台への `TrackerAdded` も、つなぎ直しになる。`HostAssigned` の 2 回目は吸収されず、`Held` から `Missing` になる。
  8. 状態の行（`Status`）: 地図同期中の台に `connected` かつ `map_ready` → 位置取り直し中。追跡中の台に同じ行が何回来ても、状態も数も変わらない。つながっている台に `connected: 0` → 状態は変わらず、順序の合わない出来事が 1 増える。見ていない番号に `connected: 1` → `inferred` で足される。`connected: 0` → 足されない。
  9. `SerialKnown`: 番号にシリアル番号が付き、`TrackerRemoved`・`DongleRemoved`・`ServerStarted`・`ServerStopped` で外れる。同じシリアル番号がほかの番号に来たら、前の番号から外れる。
  10. `ServerStarted`、`ServerStopped` で全台が未接続になる。`ServerStopped` で PC の Wi-Fi 情報が `Unknown`。接続の直後の `Pose(Lost)` と `Pose(Other)` は、状態を変えず、順序の合わない出来事にも数えない。
  11. **接続の経路と `DongleRemoved`**（レビューの指摘）: `TrackerAdded(1, usb = true)`（追跡中、シリアル番号つき）と、`TrackerAdded(2, usb = false)`、`TrackerAdded(3, usb = false)` のあとの `DongleRemoved` で、2 と 3 だけが未接続になり、1 は状態・役割・シリアル番号・`since` がそのまま。推し量って足した台（`link` が `Unknown`）は `DongleRemoved` で切れる。その台に `Status(…, usb = Some(true))` が来ていれば `Usb` になり、切れない。`link` が決まった台の `Status` の `usb` では変わらない。
  12. **電池**（5.1。#27）: `Status(N, connected, battery = Some(b))` で N の `battery` が b になり、`battery = None` の行では前の値が残る。`TrackerRemoved`、`DongleRemoved`（ドングル経由の台だけ。USB の台は残る）、`ServerStarted`、`ServerStopped`、つなぎ直し、`TrackerAdded` で、なしになる。推し量って足した行は、足した `Status` の `battery` を持つ。
  12a. **VIVE Hub の番号**（5.1。PR #37 のレビューのあとで足した。U-18）: `SerialKnown(2, S, Some(1), …)` で番号 2 の行の `hub_number` が 1。同じ接続の `SerialKnown(2, S, None, …)` では 1 のまま、`SerialKnown(2, S, Some(3), …)` で 3。`TrackerRemoved`、`DongleRemoved`（ドングル経由の台だけ）、`ServerStarted`、`ServerStopped`、つなぎ直し、`TrackerAdded` で、なしになる。S が別の番号 4 に来たら（`SerialKnown(4, S, Some(2), …)`）、番号 2 の行の `hub_number` もなしになり、4 の行が 2 になる。
  13. **台帳**（5.4。#27）: `SerialKnown(2, S, Some(1), Some("v1"))` で S の行ができる。そのあとの `SerialKnown(4, S, None, None)` で、行は 1 つのまま、版 v1 が残る。`SerialKnown(4, S, Some(2), Some("v2"))` で v2 に変わる。台帳の行に VIVE Hub の番号を持たない（12a のスロットの行で持つ）。`TrackerRemoved`、`ServerStarted`、`ServerStopped` で行が残る。`rebuild` で空になる。16 台が入っているときに 17 台目を足すと、つながっていない台のうち `last_seen` の最も古いものが消え、つながっている台は消えない（つながっていない台がなければ、`last_seen` の最も古い台を消す。スロットは 5 つなので、ふつうは起きない）。
  14. **画面に並べる台**（5.4。#27）: (a) 3 台と 5 台のそれぞれで、台帳の台が 1 台ずつ並び、`key`、`conn`、`hub_number`、`connection`、`battery`、`phase`、`host` が、シリアル番号の付いたスロットの行の値になる。(b) **ホストの交代**: `HostAssigned(2)` のあと `HostAssigned(4)` で、`host` が 2 の台から 4 の台に移り、ほかの値は変わらない。(c) **スロットの使い回し**: シリアル番号 S1 の台が番号 2 で切れ、S2 の台が番号 2 でつながる → S1 は `Disconnected`（`key` なし、`conn` なし、`hub_number` なし、`battery` なし、`host = false`）、S2 は `key = 2` で、`conn` が S2 の接続。(d) **仮の台**: `TrackerAdded(3)` のあと、`SerialKnown(3, …)` の前は、`serial` のない台が 1 つ並ぶ（`key = 3`、`conn` がその接続）。`SerialKnown(3, S3, …)` のあとは、仮の台が消え、S3 の台が 1 つだけ並び、その `conn` は仮の台の `conn` と同じ。(e) `link` が `Unknown` の台は `connection = Dongle`、`Usb` は `Usb`。(f) `DeviceView` に、スロットの番号以外で、内部の状態の名前の文字列を持たない（`phase` は型のまま）。
  15. **ドングル**（5.5。#27）: 5.5 の表の行ごと。`ServerStarted` で `Unknown`、続く `DongleAdded` で `Connected`。`DongleReport(false)` で `Disconnected` になり、トラッカーの状態は変わらない。`Unknown` と `Disconnected` のどちらでも、`TrackerAdded(N, usb = false)` が来ると `Connected`。`usb = true` では変わらない。
  16. **VIVE Hub のサーバー**（5.6。#27）: `Unknown` → `ServerStarted` で `Running` → `ServerStopped` で `Stopped`。`Stopped` のあとのほかの出来事（`TrackerAdded`、`Status` など）では変わらない。`rebuild` で `Unknown`。
- **順序**: M1-3 のあと。

### M1-6 検出の規則とエンジン

- **範囲**: `crates/openut-core/src/{rules.rs（または rules/ の下）,engine.rs}`、`crates/openut-hublog/src/replay.rs`（エンジンをつなぐ。ドライバーのファイルを受け、時刻の順に合わせる）、`tests/replay.rs`（合成のケースの `#[ignore]` を外し、足したケースの関数を足す。ケースのディレクトリから `ViveTrackerDriver_*.txt` も読む）、`tests/replay/*/expect.toml`、`tests/replay/synthetic-r1-gap-then-fault/`、`tests/replay/synthetic-r4-power-off/`。
- **中身**: R1〜R5（6.2）、エピソード、`Detection`（3.6）、`EngineConfig`（6 章のしきい値の既定値と上書き。`rules.r1.gap_s`、`rules.r3.clients`、`rules.r4.server_s`、`rules.r4.exclude_power_off`、`rules.r4.notice_before_s`、`rules.r4.notice_after_s`、`state.dup_ms` を含む。初版の `rules.r4.notice_wait_s` はやめた）、`Engine`（3.7。同じ時刻の入力と期限の順、`rebuild`。ドライバーの記録は時刻を進めず、`PowerOffNotice` は元の時刻のまま覚える。`driver_needed_through`。R4 の予告と接続の対応〔6.2〕。接続は M1-5 の `conn` で、つなぎ直しで区切る。R1 の案内する台は、ホストの接続で決める〔6.2〕。`Engine::new(cfg)` は名前の表を受けない〔#27〕）。`Detection` の `serial`（3.6。#27）と `conn`（3.6。PR #37 のレビューの指摘。R1 は `Escalated` でも決め直す）。`openut_hublog::replay` は、2 つの列で 1 つのずれの補い（F-2 の `OffsetFill`。サーバーの列の最初の記録で基準を決める）を通してから、2 つの列を合わせる。この項目では `Notice` を作らない（M1-7）。合成のケース `synthetic-r1-gap-then-fault` と `synthetic-r4-power-off`（`m1-watcher.md` の 10.1）を、見本の形の行で足す。
  - **既定は PM の判断の前の形**: `rules.r3.clients = false`（R3 はホストだけ）、`rules.r5.enabled = false`（R5 は切る）、`rules.r4.exclude_power_off = true`（予告のあった切断を除く。オーナーの確かめの前の仮）。判断のあとは既定の値を変えるだけで済むようにする（3 章）。
- **受け入れ条件とテスト**:
  1. 規則ごとに: `Raised` の `at` が、条件を満たした時刻（期限の規則〔R2〜R5、R1 の `Escalated` と `Ended`〕では期限の時刻）になる。回数の規則は、回数が 1 少ない、時間が 1 ms 足りないと出ない。`Escalated`（R1）、`Resolved`、`Ended` が 6.2 の条件で出る。
  2. **期限ちょうど**（6.1、3.7）: R2 で `ClientLink` からちょうど `r2.wait_s` の時刻に地図を受け取ると出ない。1 ms 遅れると、`at` が期限の時刻の `Raised` が出る。R3（ホストが期限ちょうどに追跡中になる）、R5（期限ちょうどに追跡中に戻る）、R1 の `Escalated`（期限ちょうどに直る）と `Ended`（最後の問い合わせからちょうど `r1.quiet_s` の時刻に次の問い合わせ）でも同じ。
  3. R1: エピソードのあいだに `HostAssigned` が来ても、2 回目の `Raised` が出ない。`Held` のあいだは、断られた問い合わせが何回あっても出ない。案内するトラッカーが、最後に `HostAssigned` された番号になる。**状態の行から始まる**（`HostAssigned` を見ていない。Wi-Fi 情報は `Unknown`）: `Status(2, connected, host = true)` と、ほかの台の状態の行のあとに、断られた問い合わせが 0.55 秒おきに 60 秒続くと、`Raised` が 1 回出て、案内するトラッカーが 2 になる。`DongleRemoved` で、USB の台が残っていても、列が捨てられ、エピソードが `Ended` になる。**番号の使い回し**（6.2 の R1 の「案内するトラッカー」。レビュー〔2 回目〕の指摘）: `HostAssigned(2)`（A）、Wi-Fi 情報が `Missing`、`TrackerRemoved(2)`、`TrackerAdded(2)`（B）、断られた問い合わせが 0.55 秒おきに 60 秒続く → `Raised` が 1 回で、`tracker` が `None`、`serial` も `None`（`{H}` は「ホストのトラッカー」になる。B の呼び名にならない。M1-7）。A のシリアル番号が分かっていれば（台帳にある。#27）、`tracker` が `None` で、`serial` が A のシリアル番号（`{H}` が A の呼び名になることは M1-7 で確かめる）。A が同じシリアル番号で番号 4 につなぎ直していれば、`tracker` が 4 で、`serial` が A のシリアル番号。つなぎ直し（5.1）でホストの接続が終わった場合も、使い回しと同じになる。ホストの接続が続いていれば、今までどおり `tracker` がその番号。**列**（6.2 の R1 の例の表の 4 行をそれぞれテストにする）: 2 回のあと 61 秒空いて 1 回では出ない。`r1.gap_s` ちょうどの空きは同じ列、1 ms 超えると新しい列。0.55 秒おきに 60 秒続くと 1 回だけ出る。`Ended` のあとの 1 回では出ず、続けば新しいエピソードになる。`HostAssigned`、`HostWifiObtained`、`WifiAssigned`、`ServerStarted`、全台の未接続で、列が捨てられる。
  4. R4: 直前 12 秒の命令（番号なしの `PcCommand` を含む）、つながっていた時間が 30 秒未満、前後 2 秒のほかの切断、前後 5 秒の `ServerStarted`・`ServerStopped`・`DongleRemoved`、切れた接続の予告（`PowerOffNotice`）、`usb` の切断のそれぞれで出ない。境目: 切断のちょうど 12 秒前の命令、ちょうど 2 秒あとのほかの切断、**ちょうど 5 秒あとの `ServerStarted`**、切断のちょうど 15 秒前とちょうど 1 秒あとの予告で出ない。5 秒と 1 ms あとの `ServerStarted`、15 秒と 1 ms 前の予告、1 秒と 1 ms あとの予告では出る。`Raised` の `at` が切断から 5 秒あと。`rules.r4.exclude_power_off = false` で、予告があっても出る。切断の行が 3 ms おいて 2 回あっても、R4 は 1 回だけ。シリアル番号が分かっている台の `Resolved` が、別の番号での同じシリアル番号の `SerialKnown` で出る。
  4a. **予告と接続の対応**（6.2。レビューの指摘）: (a) **番号の使い回し**: A が t に予告なしで切れ、B が t + 3 秒に同じ番号でつながり、t + 8 秒に B の予告が出る → A の R4 が出る（`rules.r4.notice_after_s = 10` に広げても出る）。B が t + 0.5 秒につながり、t + 0.8 秒に予告が出る場合も、A の R4 が出る（予告は B のもの）。(b) **同じ接続の予告が遅れて届く**: A の予告（時刻 t、同じミリ秒）を、B の `TrackerAdded`（t + 3 秒）より後に `apply` しても、A の R4 は出ない（ログの時刻で対応を決める）。(c) 前の接続 Z の予告（Z の切断と同じミリ秒）は、そのあと同じ番号でつながった A の切断を除かない（`r4.min_connected_s` と `r4.notice_before_s` を、A の接続が予告から 15 秒以内に切れるように変えて確かめる）。(d) 予告の時刻が、A の終わりと B の始まりの両方と同じなら、A の予告。
  4d. **つなぎ直しで接続を区切る**（5.1、6.2 の「接続」。レビュー〔2 回目〕の指摘）: A が番号 2 で 10 分つながり、切断の行がないまま、a に `TrackerAdded(2)`（B。前の `TrackerAdded` から 1 秒を超える）が来る。(a) B が a + 3 秒に予告なし・命令なしで切れる → R4 が出ない（条件 2 の長さは a から 3 秒）。(b) B が a + 40 秒に切れる → R4 が出る。(c) `r4.min_connected_s` を 10 秒にして、B が a + 12 秒に切れる。a ちょうどの時刻の予告（A の接続のもの）があっても、B の R4 が出る（予告は切断の 12 秒前で、範囲の 15 秒に入るが、別の接続のため）。a + 5 秒の予告なら B のもので、R4 が出ない。(d) a では R4 を判定しない（A の接続の終わりに `Raised` が出ない）。(e) A の接続で `Raised` していた R5（`rules.r5.enabled = true`）と、待っていた R2・R3 が、a で `Ended` になる（待ちがやめられる）。(f) a のときに、N のほかに未接続以外の台がなくても、R1 の列が捨てられない。
  4b. **ドライバーの記録は時刻を進めない**（3.7）: いまの時刻より後の時刻の `PowerOffNotice` を `apply` しても、いまの時刻が変わらず、期限も判定されない。いまの時刻より前の時刻の `PowerOffNotice` は、`time_regressions` に数えられず、元の時刻で覚えられる。`note_line` でドライバーの行を渡しても、時刻が進まず、その行の時刻の日の `source.driver.lines` に数えられる。
  4c. **`driver_needed_through`**: 判定を済ませていない R4 の期限（`exclude_power_off = true` のとき）と、いまの日の最後の時刻（次の日の 0 時の 1 ms 前）の早いほうを返す。`exclude_power_off = false` では R4 の期限を返さない。判定を済ませたら、次の期限になる。
  5. R3 と R5 が、`tracked_since_connect` で分かれる（同じ喪失で両方は出ない）。R3 のホストの始まりが `HostAssigned` の時刻。**既定**（`rules.r3.clients = false`）では、クライアントの位置合わせが終わらなくても R3 が出ず、ホストでなくなったら待ちをやめる。`rules.r3.clients = true` でクライアントにも出る。`inferred` のトラッカーには出ない。
  6. `EngineConfig` で、しきい値を変えると結果が変わる。**既定**（`rules.r5.enabled = false`）で R5 が出ない。`true` で出る。
  7. エンジン: 期限が近い順に判定される。同じ期限は規則の順、トラッカーの番号の順。時刻が戻る出来事が、いまの時刻として扱われ、数えられる。
  8. **見張りと再生で同じ**: 同じ入力の列を、(a) `apply` だけで流したときと、(b) 500 ms ごとの `advance_to` を挟み、さらに期限とちょうど同じ時刻の `advance_to` も挟んで流したときで、`Detection` の列（規則、トラッカー、段階、`at`）が同じ。2 と 4 の境目のケースを含める。
  9. `rebuild`: 状態、エピソード、期限、いまの時刻が捨てられ、新しく作ったエンジンと同じ結果になる（同じ入力を流して比べる）。
  9a. **ずれの読めない行**（4.3 の「ずれの補い」。レビューの指摘）: `openut_hublog::replay` に、サーバーの 3 行（地方時 14:00:00 `+9`、14:00:01 ずれなし、14:00:02 `+9`）を、R2 の期限（`ClientLink` から 30 秒）が 14:00:20 にある状態で流すと、`time_regressions` が 0、R2 が出ず（期限の前のまま）、`bad_offset` が 1。同じ 3 行の真ん中を `+5.5` にしても同じ。**2 つの列の時間の基準**（4.3 の例 2。レビュー〔2 回目〕の指摘）: サーバーの最初の記録が `+9`、ドライバーの最初の記録（予告の行）にずれの語がなく、そのあとはどちらも `+9` のとき、予告と同じ地方時の切断の R4 が、予告で除かれる（`power_offs` が 1）。サーバーの最初の記録にずれの語がなく、ドライバーは全部 `+9` のときも、同じく除かれる。
  10. 再生テスト: `synthetic-r1-second-host-command` で R1 がちょうど 1 回、`synthetic-normal-start` で検出なし、`synthetic-utf16` で元のケースと同じ結果、`synthetic-r1-gap-then-fault` で R1 がちょうど 1 回（不具合のあとの時刻）、`synthetic-r4-power-off` で R4 がちょうど 1 回（予告のない台）。
  11. 再生で、サーバーとドライバーのファイルが時刻の順に合わせて流れる（ドライバーのファイルを外すと、`synthetic-r4-power-off` の R4 が 2 回になる）。
  12. **`Detection` の `serial`**（3.6。#27）: R2・R3・R5（`rules.r5.enabled = true`）の `Raised` で、その番号の行のシリアル番号。R4 の `Raised` で、切れた接続のシリアル番号（切れたあと、スロットの行から外れていても）。シリアル番号が来る前の台では `None`。`Escalated`・`Resolved`・`Ended` は、`Raised` の値を引き継ぐ（あとで分かっても変えない）。R1 は 3 の番号の使い回しのとおり。
  12a. **`Detection` の `conn`**（3.6。PR #37 のレビューの指摘）: R2・R3・R5 の `Raised` で、その番号のいまの接続（`ConnId`）。R4 で、切れた接続。R1 は、案内する台が 6.2 の 1 ならホストの接続、2 でつながっていればその番号のいまの接続、ほかは `None`。`Resolved`・`Ended` は、そのエピソードの最後の値を引き継ぐ。**R1 の `Escalated` で決め直す**: 番号 2 の、シリアル番号の分からないホストで `Raised`（`serial` なし、`conn` = c）のあと、同じ接続で `SerialKnown(2, S)`、そのあと `TrackerRemoved(2)` と別の台の `TrackerAdded(2)`（接続 d）が来て、R1 が続いたまま `Escalated` になる → `Escalated` の `serial` が S、`conn` が `None`、`tracker` が `None`（S はつながっていない）。S が番号 4 でつなぎ直していれば、`tracker` が 4、`conn` が 4 のいまの接続。`expect.toml` の比較には `conn` を使わない。
- **順序**: M1-2 と M1-5 のあと。3 章の F-3 のあと（合成のケースが見本の形になっていること）。

### M1-7 案内の配信の決まりと文言

- **範囲**: `crates/openut-core/src/{notice.rs,text.rs,display.rs}`、`crates/openut-core/src/engine.rs`（`Notice` を出力に足す。続いている案内〔欄〕を返す）。
- **中身**: 7.1 の配信の決まり（push の表、同じ案内を繰り返さない決まり〔同じ台は、2 つのエピソードの `serial` がどちらも分かれば `serial`、でなければ番号 N。#27〕、間隔とまとめ、追いつき、出す前に終わったものを捨てる）と、7.1 の文言の表（#27 で直した意味。使わない言葉）。`Notice` の `conn`、`text`（`NoticeText`。呼び名の穴を残した文言）、`target`（出したときの呼び名）と、穴に呼び名を入れて文字列にする関数（3.6。PR #37 のレビューのあとで足した）。トラッカーの呼び名（3.2。末尾は台帳から、VIVE Hub の番号はいまの接続の値から作る。末尾の長さ、読めないときの形。名前の表は受けない）。**表示の写し**（`openut_core::display`。7.4。#27）: `WatchHealth`、`Overall`（正常・待機中・異常の 3 つ）、`UserState`、手引きの段、`Line`、`DisplayModel`（`greyed`、`pick_folder` を含む）、`DeviceRow` と、`view` の関数（利用者に見せる状態と案内の当て方〔`serial`、なければ `conn`〕、全体の 8 つの順、止まったときの見せ方、手引き、トラッカーの項目と並べ方〔シリアル番号の順〕、欄の文言をこの写しの呼び名で作ること、仕組みの側の不具合の 1 行）。`Line` から文言を作る関数（`text`）。
- **受け入れ条件とテスト**:
  1. 7.1 の push の表の、規則と段階の組ごとに、`push` の値が表のとおり。
  2. R2 の `Ended` から 5 分以内の同じトラッカーの `Raised` は push しない。5 分ちょうどのあとは push する。R4 は push から 5 分以内の 2 回目を push せず、欄の案内に回数が付く。R1 は 3 分おきの 3 つのエピソードを 3 回とも push する。
  3. 10 秒以内に来た 3 つの push が、優先度の最も高い 1 つと「ほか 2 件」になる。
  4. 追いつきのあいだは push しない。追いつきが終わった時点で続いているエピソードだけが push される。
  5. 出す前に `Resolved` になった push は出ない。
  5a. **再構築のあと**（7.1）: push 済みで続いていた R1 のエピソードが、`rebuild` と追いつきのあとも続いていれば、push し直さない（`Escalated` を push 済みなら、それも出さない）。続いていなければ欄から消し、「直った」を出さない。規則かトラッカーが違うエピソードは、ふつうに push される。
  5b. **同じ台**（7.1。#27）: R4 で、シリアル番号 S の台が番号 2 で切れて push され、3 分後に番号 4 でつなぎ直してまた切れる → 2 回目は push しない（同じ台）。シリアル番号の分からない台は、番号で数える（今までどおり）。
  6. 文言が 7.1 の表のとおり（呼び名: 番号と末尾〔`トラッカー 1（…0001）`〕、末尾だけ、どちらもない〔「トラッカー」「ホストのトラッカー」〕。`r5.wait_s` を変えたときの「1 分」の部分）。R1 の番号の使い回し（M1-6 の 3）で、`{H}` が「ホストのトラッカー」になり、B の呼び名にならない。A のシリアル番号が台帳にあれば、`{H}` が A の呼び名になる。 push の文言（`target` で穴を埋めたもの）は、あとで台帳や VIVE Hub の番号が変わっても変わらない。
  6a. **使わない言葉**（7.1。W-10）: 文言の関数が返すすべての文字列（7.1 の表の全部の組、手引きの G1〜G5、7.4 の仕組みの側の不具合の 1 行、トレイの 1 行）に、7.1 の「使わない言葉」が 1 つも入っていない（テストの中に言葉の一覧を持って、全部の文字列を調べる）。部位の言葉（腰、足、胸、手首など）も入っていない（W-12）。
  7. **呼び名**（3.2。W-12）: 末尾が 4 文字。2 台の末尾 4 文字が同じなら、全台が 5 文字（重ならないまで延ばす）。4 文字より短いシリアル番号は全体。同じ台をスロットの番号 2 でつないだときと 4 でつないだときで、VIVE Hub の番号が同じなら呼び名が同じ。**つながっていない台には VIVE Hub の番号が付かず、末尾だけ**（U-18。PR #37 のあとで直した）。つなぎ直して VIVE Hub の番号が 1 から 3 に変わると、番号の部分だけが変わる。ホストになっても、ホストでなくなっても、呼び名が同じ（どちらもないときの「ホストのトラッカー」を除く）。3 台と 5 台の台帳で、呼び名の末尾が台数で変わらない（末尾が重ならないかぎり）。
  8. **表示の写し**（7.4。W-10〜W-12。PR #37 のレビュー〔1 回目〕の指摘で直した）:
     - (a) **利用者に見せる状態**の表の行ごと: R4 の案内が続いている未接続の台が異常（シリアル番号で）。R1 の案内する台が異常で、Wi-Fi 情報を待つクライアントは待機中。R1 で案内する台が決まらないとき、どの台も異常でなく、全体が異常。案内のない位置取り直し中・地図同期中・接続・Wi-Fi 待ちの台は待機中。追跡中は正常。未接続は `None`。R4 の `Resolved` の 30 秒のあいだは異常にしない。
     - (a2) **案内の当て方**（7.4。レビューの指摘の 4）: 途中から読み始め、番号 2 の仮の台（接続 c）がホスト（報告）で R1 が出る（`serial` なし、`conn` = c）→ 仮の台が異常。同じ接続で `SerialKnown(2, S)` → 仮の台が消え、S の枠が異常のまま。そのあと S が番号 2 で切れ、別の台 B が番号 2 でつながる（接続 d）→ R1 が続いていても B の枠は異常でなく、全体は異常。シリアル番号の分からない台の R4 は、どの枠も異常にしない。
     - (b) **全体**の表の行ごと（上から順に当てはまること。`overall` に、正常・待機中・異常のほかの値がない）: 追いつきのあいだ → 待機中、`line` なし、`greyed`。`Stalled` と R1 が同時（`hub_process` が `Some(true)` か `None`）→ 待機中、`line` が `Health(Stalled)`、`greyed`（R1 の台の枠と欄は、値として残る）。`NotFound` → 待機中、G1、`pick_folder = true`。`hub_process = Some(true)` で `HubState = Stopped` → 止まっていない（台の値をそのまま見せる）。`None` で `Stopped` → G1 と、止まったときの見せ方。異常の `line` が、優先度の最も高い案内（R1 と R4 が同時なら R1）。全台が正常 → 正常で `line` が G5。台が 0 台 → 待機中で G3。
     - (b2) **VIVE Hub が止まった直後**（レビューの指摘の 2）: ログは `Running` のままで、3 台がつながり（1 台がホスト、電池あり）、R1 と R4 の案内が続いていて、具合は `Ok`。ここで `hub_process = Some(false)` → 待機中、`line` が G1、`greyed = false`、`pick_folder = false`、止まったときの見せ方（(h)）。**`Stalled` が重なる**: 同じ状態で具合を `Stalled` にしても、結果が同じ（`Health` にならない）。追いつきのあいだの `Some(false)` でも G1。`Some(false)` と `NotFound` が重なると、G1 で `pick_folder = true`。
     - (c) 手引き: G1〜G4 の済んだ条件。ドングルが `Unknown` で G2 が済み、`Disconnected` で済んでいない。いまの段より後の段は `done = false`（VIVE Hub が止まっているとき、G2〜G4 が済んでいない）。G4 の `progress` が（正常の台数、未接続でない台数）。
     - (d) トラッカーの項目の、読めないときの値: 未接続の台の電池は `None`、呼び名に VIVE Hub の番号がない。仮の台は呼び名の末尾がなく、シリアル番号が `None`。版がなければ `None`。ホストの接続が終わった台に `host = false`。
     - (e) **並べ方**: シリアル番号の順。仮の台は最後。7.4 の「並べ方」の例の表の 5 行を、それぞれ並びと呼び名のテストにする（切れた台の番号がなくなる、VIVE Hub の番号を振り直しても並びが同じ、ホストを替えても同じ、末尾の重なる台が加わると全台が 5 文字になり、加わった台がシリアル番号の位置に入る）。3 台と 5 台。
     - (f) 仕組みの側の不具合の 1 行が、具合ごとに 7.4 の表のとおり（`Unreadable` → OpenUT を起動し直す、`Stalled` → VIVE Hub を起動し直す、`FormatMismatch` → OpenUT を新しい版にする）。
     - (g) `DisplayModel` と `DeviceRow` に、内部の状態（`Phase`）、PC の Wi-Fi 情報、スロットの番号（`TrackerKey`）、接続の番号（`ConnId`）、経過時間、集計の値の欄がない（レビューで見る）。
     - (h) **止まったときの見せ方**（7.4）: 台帳の台がすべて `connection = Disconnected`、電池なし、`state = None`、`host = false`、呼び名が末尾だけ。仮の台は並ばない。`faults` が空で、どの台も異常でない。ドングルが `Unknown`。手引きのいまの段が G1 で、G2〜G5 が `done = false`。
     - (i) **`pick_folder`**（レビューの指摘の 3）: 具合が `NotFound` のときだけ `true`。具合が `Ok` で、`HubState = Stopped`（`hub_process = None`）のときと、`hub_process = Some(false)` のときは、どちらも G1 で `false`。
     - (j) **欄の文言**（7.4。レビューの推奨）: (a2) の R1 で、`SerialKnown` の前の欄の行が「ホストのトラッカー」の文言、あとの行が S の枠と同じ呼び名の文言。S の台が枠に並んでいる状態で、末尾の重なる台が台帳に入ると、同じ写しで枠と欄の呼び名がどちらも 5 文字になる。当たる台がない案内（シリアル番号の分からない R4）の行は、`target` の呼び名。
- **順序**: M1-6 のあと。

### M1-8 日ごとの集計

- **範囲**: `crates/openut-core/src/summary.rs`、`crates/openut-core/src/engine.rs`（集計をつなぐ）、`crates/openut-core/Cargo.toml`（`serde` を機能 `serde` の任意の依存に、`serde_json` をテストだけの依存に）、`tests/replay/*/expect.toml`（`[summary]`）、`tests/replay/synthetic-midnight/`、`tests/replay/synthetic-midnight-no-prev/`、`tests/replay.rs`（2 つのケースの関数）。
- **中身**: 8 章の値と、8.1 の日の区切りと数え方（行の時刻の 0 時で分ける、行と行のあいだを前の状態で数える、`summary.gap_s` を超える空きは `unobserved_s`、最後に読んだ行の時刻まで、日を閉じる時、`observed_until`）。トラッカーの行はシリアル番号ごと（1 回の接続の分を、その接続で分かったシリアル番号の行に足す。分からなければ番号ごとの行）、`slots`、`hub_number`（#27。初版の `name` は替えた。8 章）、`power_offs`、`source` の `meta`・`bad_offset`・`driver`。JSON の形は、`serde` の機能を付けたときに 8 章の形で書き出せるようにする（書き出しのファイルの I/O は M1-9）。
- **受け入れ条件とテスト**:
  1. 合成の出来事の列で、`connected_s`、`in_use_s`、`tracking_s`、`tracking_ratio`、`first_pose_s`、`losses`、`long_losses`、`host_starts`、`faults`、`detections` が手で計算した値になる。
  2. **日の区切り**: 0 時をまたぐ状態が、0:00:00.000 で 2 つの日に分かれる（新しいファイルの最初の行の時刻ではない）。前の日のファイルにある 0 時より後の時刻の行は、新しい日に数える。前の日は、次の日の 0 時以上の時刻の行を適用したときに `final = true` になり、それより前は `false`。行が 1 行もない日の集計は作らない。
  2a. **8.1 の例の表**の 4 つの行が、表の値になる（連続して流す、起動し直しを模して新しいエンジンに 2 つのファイルを追いつきとして流す、2 つのファイルの再生、11/28 だけ）。表のあとの「空きが `summary.gap_s` を超える場合」の値（11/28 の 00:12:00 と 00:21:00 の行を除いたとき。前の日のファイルがある・ない、の両方）と、最初の行が 00:10:00 のときの `unobserved_s` も確かめる。
  2b. **数えない時間**: `summary.gap_s` ちょうどの空きは状態に数え、1 ms 超えると `unobserved_s` に入る。最後の行のあと `advance_to` で時刻を進めても（`flush_s` と同じ）、時間の値が増えない。`flush_s` のあいだに出た検出は `detections` に数えられる。
  2c. **再構築**: 途中まで流したエンジンで `rebuild` し、同じファイルをもう一度流すと、集計が 1 回だけ流したときと同じ（回数が二重にならない）。
  3. `in_use_s` が 0 のとき `tracking_ratio` が `null`。**この境界は、JSON の出力のテストで確かめる**（`expect.toml` には `null` の表記を足さない。Issue #20 の PM の判断）。
  3a. **シリアル番号の行**: 同じシリアル番号のトラッカーが、番号 2 でつながって切れ、番号 4 でつなぎ直す → 1 つの行に両方の接続の時間と回数が入り、`slots` が `[2, 4]`。`SerialKnown` が接続から 8 ms あとに来ても、その 8 ms もその行に入る。シリアル番号が来ない接続は、番号ごとの行（`serial: null`）に入る。**つなぎ直し**（5.1）: シリアル番号 S1 の台が番号 2 でつながり、切断の行がないまま a に `TrackerAdded(2)` が来て、S2 の `SerialKnown(2, S2)` が来る → S1 の行の接続の時間は a まで、S2 の行は a から。`sessions` は S1 と S2 に 1 ずつ。
  3b. 古い日のファイルの最後の、次の日の時刻の記録（メモの 2.5）で前の日が閉じ、その記録は次の日に数えられる。
  3c. **ずれの読めない行で日が飛ばない**（レビューの指摘）: 23:59:58 `+9`、23:59:59 ずれなし、00:00:00 `+9` の 3 行で、23:59:59 の行が前の日に数えられ、前の日は 00:00:00 の行で閉じる（23:59:59 の行で閉じない）。時間の値が、3 行とも `+9` のときと同じ。
  3d. **USB の台と `DongleRemoved`**: USB の台 1 台とドングル経由の台 2 台がつながっているときの `DongleRemoved` のあと、USB の台の `connected_s` と `tracking_s` が増え続け、その台の行（シリアル番号）に入る。ドングル経由の 2 台は、そこで接続が終わる。
  3e. **ドライバーの行の日**: ドライバーの行は、その行の時刻の日の `source.driver.lines` に数えられる（サーバーの行より先の日の時刻でも、その日が始まってから、その日に入る）。ドライバーの行だけでは、時間の値も `observed_until` も変わらない。
  4. 4.7 の数が `source` に入る。
  5. JSON に書き出したものが 8 章の形（キーの名前と入れ子。`hub_number` を含み、`name` を含まない）になる（文字列で比べるテスト）。トラッカーの行の `hub_number` が、その日の最後に分かった番号（その日のうちに 1 から 2 に変わった台は 2。分からなければ `null`）。
  6. 再生テスト: 合成のケースの `expect.toml` に `[summary]` を書き、一致する。`synthetic-midnight` と `synthetic-midnight-no-prev` が 8.1 の表の値になる。
- **順序**: M1-6 のあと（M1-7 と並べてよい）。

### M1-9 設定、ログのフォルダーを探す、見張りのループ、コマンド

- **範囲**: `src/{main.rs,cli.rs,config.rs,paths.rs,discover.rs,watcher.rs,summary_io.rs,hub_process.rs}`、`src/notify/{mod.rs,console.rs}`、`Cargo.toml`（`serde_json` を足す。`sysinfo` を Windows だけの依存に足す。#27）。
- **中身**:
  - 設定（9.1。トラッカーの名前の設定はない。初版の `[trackers]` は読み飛ばす。#27）の読み込みと書き直し、保存場所（9.3）、フォルダーを探す（9.2。既知の候補 `%ProgramData%\HTC\ViveSoftware\ViveUltimateTracker\Log`）。
  - 見張りのループ（2 章、4.6）: `LogSource` と時計と眠りを引数で受ける形にして、テストでは偽物を渡す。500 ms ごとの追記の読み取り（サーバー、ドライバーの順に読み、M1-4 の合わせる部品に、エンジンの `driver_needed_through()` を上限として渡す。渡した記録で上限が変わったら、もう一度渡す。4.6 の「ドライバーを待つ」）、`advance_to(min(見張りの時刻, 上限))`（上限は、W_drv がそれを超えていないときだけ）、読んだまとまりごとの遅れ（壁時計 − 最後の行の時刻。サーバーとドライバーのそれぞれ）をアプリのログに残す（L を決めるため。4.6）、`_N` のファイルを見つけたらアプリのログに 1 回だけ残す、出力の振り分け、60 秒ごとの集計の書き出し（追いつきのあいだは書かない）、見張り役の具合（7.4 の「仕組みの側の不具合」。下の項目）。`Tailer`（M1-4）が返す「再構築する」で、閉じていない日の集計を `YYYY-MM-DD.before-rebuild-N.json` に書いてから `Engine::rebuild` と `set_catching_up(true)`、「読み終えた」で d₀ を測り（M1-4 の `Watermark`）、そこまでに読んだサーバーの記録をすべてエンジンに渡し終えたときに `set_catching_up(false)`（ドライバーを待って持っている記録があれば、渡し終えるまで追いつきを続ける。判定がまだの R4 の切断は、追いつきの終わりで判定せずに持ち越す。4.5）、「次のファイルに移った（日が飛んだ）」で集計を書いてから `rebuild`（4.1、4.5）。閉じた日の集計は、`final: true` のファイルがすでにあれば書き直さない（4.5、9.3）。
  - **見張り役の具合**（`WatchHealth`。7.4 の「仕組みの側の不具合」。#27）を決め、表示の写し（M1-7 の `display::view`）に渡す: `NotFound`、`Unreadable`（3 回続けて失敗）、`Stalled`（未接続でない台があるのに 10 分伸びない。追いつきの終わりには、壁時計 − サーバーで最後に読んだ記録の時刻から数え始める）、`FormatMismatch`。理由と数は、アプリのログにだけ書く。
  - **VIVE Hub のプロセスの確かめ**（5.6。#27。Windows だけ）: 10 秒ごとに、`sysinfo`（0.40.4。`default-features = false, features = ["system"]`。Windows だけの依存）でプロセスの名前の一覧を取り、設定の `watch.hub_process_names`（既定は `ViveTrackerServer.exe`）と大文字と小文字を区別せずに比べる（比べる部分は入出力のない関数）。結果を `hub_process` として表示の写しに渡す（規則と集計には使わない）。プロセスの名前だけを更新する `sysinfo` の呼び方を docs.rs で確かめ、1 回にかかる時間を PR の説明に書く。Linux と `openut replay` では確かめない（`None`）。
  - `NoticeSink` トレイトと、コンソールの出口（`push` の案内を 1 行で出す）。
  - コマンド: `openut watch [--log-dir D] [--config F] [--data-dir D]`（画面なしの見張り。Linux でも動く）、`openut replay <ケースのディレクトリかファイル>... [--expect F] [--set 名前=値]... [--format text|toml]`（不一致なら終了コード 1）、`openut summarize <ファイル>... [--out D]`、`openut --version`。引数がないときは、Windows では M1-11 の常駐（それまでは使い方を表示）、ほかでは使い方を表示。
- **受け入れ条件とテスト**:
  1. 設定: ファイルがない → 既定値。9.1 の例が読める。形が違う → 既定値と理由（止まらない。理由はアプリのログ）。通知の切り替えとフォルダーの選択を書き直して、読み直すと同じ値。初版の `[trackers."<シリアル番号>"] name = "腰"` がある設定 → 止まらずに読め、`[trackers]` は使われない。
  2. フォルダーを探す: `log_dir` が優先される。`ViveTrackerServer_*.txt` がないフォルダーは使わない。`ProgramData` の環境変数を偽の値にして、既知の候補が見つかる。環境変数がなければ、候補を飛ばして「ログが見つからない」になる（止まらない）。
  3. 見張りのループ（偽の `LogSource` と時計）: 伸びていく合成のログで、R1 の案内がコンソールの出口に 1 回だけ出る。追いつきのあいだに終わった検出は出ない。ログが静かなあいだも、見張りの時刻で R2 の期限が判定される（L を待ってから）。
  3a. **動き続けた・起動し直した・再生した、で同じ**: 8.1 の例の 2 つのファイルを、(a) 11/27 の途中から伸ばしながら 0 時をまたいで見張る、(b) 00:40 に見張りのループを作り直して（起動し直しを模して）読み直す、(c) 同じファイルを再生する、の 3 つで、11/28 の集計と検出が同じ。(b) で、11/27 の `final: true` のファイルが書き直されない。
  3c. **ドライバーが遅れて届く**（4.6 の「ドライバーを待つ」。レビューの指摘）: 偽の `LogSource` と時計で、読み取りの回を分けて確かめる。
     - (a) **予告が範囲の終わり近く、別の回に届く**: 1 回目の読み取りで、サーバーに切断（時刻 t、予告なしで R4 の条件 1〜4・6 を満たす台）と、t + 5.1 秒の行（期限 t + 5 秒より後）が入り、ドライバーは t − 20 秒の行まで。2 回目（壁時計で 3 秒あと）に、ドライバーに t + 0.9 秒の予告が入る。R4 が出ない（再生と同じ）。1 回目のあとも、t + 5.1 秒の行がエンジンに渡されていないこと（`driver_needed_through` が t + 5 秒で、W_drv がそれ以下のため）。
     - (b) **予告が切断と同じミリ秒で、3 秒遅れて届く**: `synthetic-r4-power-off` のファイルを、ドライバーのファイルだけ 3 秒遅れて伸ばしても、R4 の結果と `at` が再生と同じ。
     - (c) **ドライバーが静か**: ドライバーのファイルが伸びないまま、偽の壁時計を進めると、期限から L（と d）たったところで R4 が 1 回出て、`at` は期限の時刻（有限の時間で進む）。そのあいだ、期限より後のサーバーの記録が渡されず、L のあとに順に渡される。
     - (d) **L より遅れて届く**: (a) の予告を、壁時計で期限から L を超えたあとに届けると、R4 が出て（再生と違う）、`driver.late` が 1 になる。
     - (e) **ドライバーのファイルがない**: 待たずに、期限を過ぎた最初のサーバーの記録で R4 が出る。
     - (f) **0 時**: 23:59:59.500 のドライバーの行が、サーバーの 00:00:01 の行より 3 秒遅れて届いても、前の日の `source.driver.lines` に入り、前の日はそのあとで閉じる。
     - (g) **起動の直後に追記される予告**（4.6 の 3。レビュー〔2 回目〕の指摘）: 切断 t（予告なしなら R4 の条件 1〜4・6 を満たす台）のあと、壁時計で t + 5.3 秒に見張りのループを作る（起動）。そのとき、サーバーのファイルは t + 5.1 秒の行まで、ドライバーのファイルは t − 20 秒の行まであり、どちらもいまのファイル。ドライバーの t + 0.9 秒の予告は、壁時計で t + 5.9 秒に追記される。R4 が出ない（再生と同じ）。追いつきのあいだも、読み終えたあとも、予告が入るまで、R4 の判定が行われない。t + 5.1 秒の行は、待ちが解けてから渡され、そのあとに追いつきが終わる。
     - (h) **読み終えたあとにサーバーが伸びない**: (g) で予告を追記しない。サーバーもドライバーも伸びないまま、偽の壁時計を進めると、読み終えてから max(0, 期限 − サーバーの最後の記録の時刻) + L（ここでは期限 t + 5 秒が最後の記録 t + 5.1 秒より前なので L）を過ぎたところで R4 が 1 回出て、`at` は期限の時刻（有限の時間で進む）。期限がサーバーの最後の記録より後（例: 起動の時点でサーバーが t + 2 秒の行まで）でも、読み終えてから 3 秒 + L を過ぎたところで出る。
     - (i) **古い期限は待たない**: 前の日のファイル（ドライバーのファイルも、次の日のものがある）の途中の切断の期限は、追いつきの中で、待たずに判定される（ドライバーを読み進めて、その時刻を超えるため）。いまのファイルの切断でも、期限がサーバーの最後の記録より L 以上前なら、遅くとも読み終えた時に判定される。
     - (j) **再構築の境目**: (g) と同じ時刻の関係で、見張りの途中に作り直し（4.1。先頭の違う中身への置き換え）が起きても、R4 が出ない（再構築の追いつきでも、同じに待つ）。
  3d. **形が違う**（7.4）: 表の種類の行が 1 つもない 2,001 件の記録で、見張り役の具合が `FormatMismatch` になる。
  3e. **止まっている**（7.4。#27）: (a) 偽の時計で、未接続でない台があるまま、サーバーのファイルが 10 分伸びない → `Stalled`。伸びたら `Ok`。台がすべて未接続なら、伸びなくても `Ok`。(b) 起動の時点で、サーバーの最後の記録が壁時計の 11 分前で、その時点の状態に未接続でない台がある（終了の行がない）→ 追いつきの終わりに `Stalled`。9 分前なら、1 分あとに `Stalled`。(c) 偽のプロセスの一覧（`ViveTrackerServer.exe` がない）→ `hub_process = Some(false)`、名前の大文字と小文字が違ってもある → `Some(true)`、設定で足した名前もある → `Some(true)`。
  3b. **作り直し**: 見張りの途中（R1 の案内を push したあと、R1 が続いているところ）で、いまのファイルを (a) 先頭が同じで行を足したものに置き換える（追記と同じに扱われ、再構築しない）、(b) 先頭の違う中身（前の世代の行の一部を含む）に置き換える、(c) 切り詰めて新しい行だけにする、(d) 消す（1 日前のファイルが残っている）。どれでも、その日の回数が二重にならず（その時点のフォルダーで起動し直して読んだのと同じ集計）、続いている R1 の案内がもう一度出ない。(b)〜(d) で、作り直しの前の集計が `before-rebuild-1.json` に残る。
  4. 集計の書き出し: 一時ディレクトリに `YYYY-MM-DD.json` ができ、書き出しの途中のファイルが残らない（一時ファイルの名前を変える形）。
  5. `openut replay`: 合成のケースで終了コード 0、期待を 1 つ消した `expect.toml` で終了コード 1。`--set rules.r1.count=1000` で R1 が出なくなる。`--set rules.r5.enabled=true` で R5 が数えられる。`--format toml` の出力を `expect.toml` として読み直せる。
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
- **中身**: 7.4 の、トレイ（3 つの見え方〔灰色のときも待機中〕、指を置いたときの 1 行、メニュー）、状態の窓（1〜6。横長。D-34）、操作の流れ F1、F2、F4、F5（F3 はやめた。#27）。見た目は、UI/UX デザイン担当の M1 のデザイン（`docs/design/ui/`）が main にあれば従う。なければ、最小の見た目で作り、デザインが入ったら直す（後続の修正の一覧に PM が足す）。
  - 最初に試作で確かめる: `tray-icon` と `eframe` を同じイベントループで動かせるか、窓を閉じたときに隠すだけにできるか、日本語の書体を Windows の書体から読み込めるか。確かめられなければ、PR を出す前に `role:pm` で報告する（設計の見直しが要る）。
  - 画面に出す中身は、M1-7 の表示の写し（`openut_core::display::view` の `DisplayModel`）だけを使う。`Snapshot` の内部の値（`Phase`、PC の Wi-Fi 情報、スロットの番号）と集計を、画面のコードから直接読まない（W-10。#27）。写しから、トレイの見え方と 1 行を選ぶ部分は、入出力のない関数にする。
- **受け入れ条件とテスト**:
  1. Windows の CI でビルドとテストが通る。
  2. 写しからトレイを選ぶ部分の単体テスト: 全体の 3 つ（正常、待機中、異常）ごとの見え方と 1 行（「正常 3/3」の台数、異常のときの見出し）。灰色（`greyed`）のときも待機中の見え方で、1 行が `Health` の文言（台数なし）。VIVE Hub が止まっているとき（`Guide(G1)`）も台数なし。
  3. 「フォルダーを選ぶ」のボタンは、写しの `pick_folder` が `true` のときだけ出る（VIVE Hub が止まっているだけのときは出ない。写しからボタンを決める部分の単体テスト）。選んだフォルダーが設定に保存され、見張り役の具合が `NotFound` から `Ok` になる（M1-9 の設定の関数を通すテスト）。
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

- **範囲**: `tests/replay/hub-<日付の範囲>/expect.toml`（名前は `m1-watcher.md` の 10.1）、`tests/replay.rs`（テスト関数を足す）。ログそのもの（サーバーとドライバーのファイル）は PM が入れる（L-2）。
- **中身**: 10.4。しきい値を 6.1 の決め方で決め、R1 がちょうど 4 回、受け入れの基準の時刻に出ることを確かめる。決めた値と、ほかの規則の検出の数、`summary.gap_s` を変えたときの `unobserved_s`（8.1）、`power_offs` と R4 の数、予告の時刻 − 切断の時刻の分布（`r4.notice_after_s` を決めるため。6.2）、5.1 の「順序の合わない出来事」の数（U-5 の確かめ）、画面に見せる項目の確かめ（#27。U-18: `model name` の N が 1 つの接続のあいだに変わった数と、`display index` + 1 と同じだった割合〔台ごとに固定でないことは、PM が数えた。`m1-watcher.md` の 4.8〕。U-19: `connected: 1` の状態の行で `percentage: 0` だった数と、接続からの時間。U-20: `Dongle is removed.` の前後の `dongle connected` の値）を PR の説明に書く（設計の表を直すかは PM が決める。数だけを書き、シリアル番号は書かない）。
- **受け入れ条件とテスト**: 再生テストで R1 がちょうど 4 回（2026-11-26 01:43、2026-11-27 22:53・22:56・22:59）。
- **順序**: M1-6 のあと、かつ PM が匿名化した実物のログを `tests/replay/hub-<日付の範囲>/`（`m1-watcher.md` の 10.1 の名前。例: `hub-2026-11-23-27`）に入れたあと（PM が依頼を出す）。

## 3. 後続の修正の一覧

設計の変更・追補に伴って、実装担当が直すもの。設計担当が足し、PM が Issue にする。

| 番号 | きっかけ | ファイル | 直すこと | 確認の条件 |
|---|---|---|---|---|
| F-1 | #18（記録の終わりは CRLF。本文の中に LF。`m1-watcher.md` の 4.2） | `crates/openut-hublog/src/decode.rs` | ファイルごとの改行の型（最初の `\n` の直前が `\r` なら CRLF の型、でなければ LF の型）。CRLF の型では `\r\n` だけで記録を切り、`\r` の付かない `\n` と、`\n` の続かない `\r` を本文に残す。LF の型は今と同じ。決めた型を返す関数（匿名化の道具が使う）。64 KiB の切りは、本文の中の LF を含めて数える | (a) UTF-16LE・BOM・CRLF で、`[...] Wait for HID data got timeout.\n` + CRLF が 1 件になり、本文の末尾が `\n`。(b) `Save map creation time: Wed Nov 25 21:00:00 2026\n.` + CRLF が 1 件。(c) LF だけの UTF-8 は今と同じに切れる。(d) `\r` と `\n` のあいだ、本文の中の `\n` と次の `\r\n` のあいだで読み取りが切れても、1 バイトずつ渡しても、同じ結果。(e) 先頭が `# Log begin at ...` + CRLF のファイルが CRLF の型になる。(f) 今のテスト（擬似乱数を含む）が通る（変える場合は理由を PR に書く） |
| F-2 | #18（行の先頭の形。`#` の行。4.3）、PR #26 のレビュー（ずれの補い。2 回目: 2 つの列で時間の基準を 1 つに） | `crates/openut-hublog/src/header.rs` | `ParsedLine::Meta`（`#` で始まる行）。`Entry::offset_known` と、ずれが読めない・ないときに 0 で続ける決まり（4.3 の 3）。`OffsetFill`（4.3 の「ずれの補い」）: **サーバーとドライバーで 1 つ**持ち、`Entry` を列（`FileKind`）つきで通す。基準のずれ B は、サーバーの列の最初の記録だけで決める（ずれが読めれば値 B₀、読めなければ「ずれなし」）。B が値なら、ずれの読めた行はその行のずれで直して、その列の「前の読めたずれ」P を更新し、読めない行はその列の P（なければ B₀）で直す。B が「ずれなし」なら、両方の列の全部の行を 0 で直す。B が未定のあいだのドライバーの記録は持っておき、B が決まったら順に直して返す（持つのは 2 万件まで。超えたら古いものから捨てて `driver.bad_offset` に数える）。終わりの処理（再生で、最後までサーバーの記録がなければ、B を「ずれなし」にして、持っていた記録を返す）。`bad_offset` に数える行を、列ごとに返す。持ち方（借用か所有か）は実装担当が決めてよい | (a) 見本の行が読める: 関数名が空（`INFO ] [13328] ==================== Log Start ====================`）、本文が空（`ERROR BiCollector::GetPcSerialNumber] [3976] `）、`::` が 2 つ・`~` の入った関数名、本文が `[2] OET OVERSPEED!` で始まる行、本文の先頭の空白 2 つ（ドングルの行）、末尾の空白が残る。(b) `# Log begin at 2026-11-28 00:00:53.613`、`# WriteTimeInterval=5000`、`# Log rotate by date at 2026-11-27 00:01:03.40` が `Meta`。(c) ずれが `+5.5` → 0・`offset_known = false`。ずれの語がない `[2026-11-27 22:53:10.123 0xF88 INFO f] [1] x` → 0・`false`、`level` が `INFO`。今の `NoHeader` のテストのうち、ずれだけがおかしい行（`... 9]`、`+5:60`）は `Entry`（`offset_known = false`）に変える。(d) `OffsetFill`: 地方時 14:00:00 `+9`、14:00:01 ずれなし、14:00:02 `+9` の 3 行が、UTC の 05:00:00、05:00:01、05:00:02 になり、2 行目の `offset` が `+9`。真ん中が `+5.5` でも同じ。(e) サーバーの最初の行のずれが読めず、2 行目が `+9` → 2 行とも 0 で直る（時刻が段にならない）。そのあとのドライバーの `+9` の行も 0 で直る。(f) `+9` のあとの `+10` → `+10` で直り、そのあとの読めない行は `+10` を引き継ぐ。(g) 作り直した `OffsetFill` は未定から始まる。(h) **片方の先頭だけ欠ける**（4.3 の例 2。レビュー〔2 回目〕の指摘）: サーバーの最初の行が `+9`、ドライバーの最初の行がずれなしで、そのあとはどちらも `+9` → ドライバーの最初の行は `+9`（B₀）で直り、同じ地方時 14:00:00 のサーバーの行とドライバーの行が同じ `at` になる。(i) サーバーの最初の行がずれなしで、ドライバーは全部 `+9` → ドライバーも 0 で直り、同じ地方時の 2 行が同じ `at` になる。(j) サーバーの最初の記録より先に渡したドライバーの記録は、その時点では返らず、サーバーの最初の記録を渡したときに、渡した順に直して返る。2 万と 1 件目で、最も古いものが捨てられ、`driver.bad_offset` が 1。サーバーの記録がないまま終わりの処理をすると、0 で直して返る。(k) **渡す順に左右されない**: 同じ 2 つの列を、サーバーを全部先、ドライバーを全部先、1 件ずつ交互、の 3 通りで渡すと、どの記録の `at` と `offset` も同じ |
| F-3 | #18（合成のケースの語句を見本に合わせる。10.1） | `crates/openut-hublog/src/replay.rs`、`tests/replay.rs`、`tests/replay/synthetic-*/`、`tests/README.md` | `ReplayCounts` に `meta` を足す（`Meta` を `no_header` に数えない）。3 つの合成のケースの行を、見本（`hub-log-samples.md`）の形に書き直す（10.1 の決まり。`# Log begin at` の見出し、関数名とレベル、`Tracker (N) - switch to HOST mode.`、番号のない `Wifi info is not yet available.`（`WARN`）と `Got host wifi info.`、`Query wifi from tracker N`、状態の行、`(SteamVR: S)(N)[K]Pose status is changed: ...`、`GetDeviceInfoAndResponse` の行）。`synthetic-utf16` は書き直した `synthetic-normal-start` を UTF-16LE・BOM・CRLF で保存し直す。各ケースの `README.md` と `expect.toml`（時刻を変えたら）。`tests/README.md` の W-1 と W-4 の行に、ドライバーのファイルと見本の行のテストを書き足す | 3 つのケースで `no_header` が 0、`meta` が見出しの行の数。`synthetic-utf16` と `synthetic-normal-start` の記録の数が同じ。`synthetic-r1-second-host-command` の `expect.toml` の R1 の時刻とトラッカー（ホストの番号）が、書き直した行と合う（README に計算を書く）。`verify.sh` が通る |
| F-4 | #27（PRD に W-10〜W-12 が足された） | `tests/README.md` | 「要件との対応」の表に、W-10、W-11、W-12 の行を足す（確かめ方とテストの場所は `m1-watcher.md` の 1 章の表: W-10 は M1-7 の表示の写しと文言の単体テスト、M1-9 の見張り役の具合、M1-11。W-11 は M1-3・M1-5・M1-7 の単体テストと、オーナーの確認〔14 章の 5〕。W-12 は M1-5・M1-6・M1-7 の単体テスト）。W-2 の行の「PC 側の Wi-Fi 情報を保つ」に、画面には出さない（7.4）ことを書き足す | 表が W-1〜W-12 と、受け入れの基準の 3 つのすべてを扱っている。`verify.sh` が通る |
| F-5 | #27（読む行を足した。`m1-watcher.md` の 4.4） | `crates/openut-hublog/src/record.rs`、`crates/openut-hublog/src/map.rs`、`crates/openut-core/src/event.rs` | **M1-3 の PR が、#27 の PR より先に main に入ったときだけ**。M1-3 の中身のうち #27 で足したもの（`SerialKnown` の `hub_number` と `firmware`、シリアル番号の文字の決まり、`Status` の `battery`、`DongleAdded`、`DongleReport`、`Peripheral data changed` を `other` にすること）を足す。M1-3 の PR に入っていれば、この行は要らない（PM が閉じる） | M1-3 の受け入れ条件の 8 |

- 順序: F-1 と F-2 は、どちらからでもよい（同じ PR でもよい）。F-4 は、いつでもよい（M1-3 の PR に入れてもよい）。F-5 は M1-3 のあと、M1-5 の前。F-3 は F-2 のあと（`Meta` を数えるため）。M1-3 は F-1・F-2 のあと、M1-6 は F-3 のあと（2 章の各項目の「順序」）。
- 匿名化の道具（#8）は、F-1 の前に作ると、本文の中の LF で記録が割れたまま書き戻す（行の形が変わる）。#8 の PR が F-1 より先に入る場合は、F-1 のあとに、道具の出力で記録の数と形が変わらないことを確かめるテストを足す（PM が #8 か F-1 の Issue に書く）。

**判断を待っている既定**（`m1-watcher.md` の 14 章）: R3 をクライアントにも当てるか（`rules.r3.clients`。PM）、R5 を足すか（`rules.r5.enabled`。PM）、電源オフの予告のあった切断を R4 から除くか（`rules.r4.exclude_power_off`。オーナーの試しのあと PM）。M1 の実装は、判断の前の既定（`r3.clients` と `r5.enabled` は `false`、`r4.exclude_power_off` は `true`）で作る。決まったら、設計担当が追補でこの表に「`crates/openut-core/src/rules` の `EngineConfig` の既定を変え、既定の値を確かめるテストを直す」という行を足す。
