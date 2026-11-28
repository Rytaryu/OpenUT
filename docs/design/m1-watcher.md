# M1 見張り役の設計

VIVE Hub を使ったまま横で動かし、VIVE Hub のトラッカー用サーバーのログを読んで、始まらない・途切れる理由と対処を、その場で知らせる（PRD の 5 章）。最終更新: 2026-11-28（画面と通知で見せるものの追補。Issue #27。PRD の W-10〜W-12。PR #37 のレビュー〔1〜3 回目〕の指摘を反映）。

## 0. 範囲と前提

- 根拠: `docs/product/PRD.md`（4〜6 章、11 章）、`docs/project/DECISIONS.md`（D-9〜D-33）、`docs/research/findings-2026-11-27.md`、`docs/research/findings-2026-11-28.md`、`docs/research/hub-log-format.md`（書式のメモ。以下「メモ」）、`docs/research/hub-log-samples.md`（見本）、`overview.md`。
- **メモ（Issue #7）に合わせて直した版**（Issue #18）。直したのは、4.1〜4.8（読み取り、行の種類）、5 章（状態）、6.2 の R1 と R4、8 章（集計）、9 章（設定、フォルダー）、10 章（再生）、14 章。メモは、オーナーの PC 1 台・6 日分・1 つの版のログによる（メモの冒頭）。ほかの版や PC で違っていても止まらないことは、4.7 で守る。
- PR #26 のレビュー（1 回目）の指摘で直したもの: ずれの読めない行の時刻（4.3 の「ずれの補い」）、ドライバーの記録を待つ決まり（3.7、4.6。R4 の照合の範囲と、追記を待つ時間を分けた）、予告と接続の対応（6.2 の R4）、ホストの根拠を Wi-Fi 情報と別に持つこと（3.5、5.3）、ドングルが抜けたときにドングル経由の台だけを切ること（5.1。接続の経路 `link`）。
- レビュー（2 回目）の指摘で直したもの: サーバーとドライバーで時間の基準を 1 つにすること（4.3 の「ずれの補い」）、起動・再構築の追いつきの終わりでも、追記中のドライバーのファイルを待つこと（4.5、4.6 の 3。d₀）、切断の行を見落としたつなぎ直しで接続を区切ること（5.1 の「接続」、6.2 の R4）、ホストの根拠を接続に結びつけ、番号が別の台に使われたら、その台の名前で案内しないこと（3.5、5.3、6.2 の R1）。
- **画面と通知で見せるものを直した版**（Issue #27。PRD の W-10〜W-12、D-34、D-35）。直したのは、1 章、3.2〜3.7、4.4、4.8、5.1、5.4〜5.6（足した）、6.2 の R1、7 章、8 章、9.1、14 章。内部の状態（W-2 の 6 つ）、PC の Wi-Fi 情報、見張り役の具合は、検出のためにこれまでどおり持ち、画面と通知には出さない（7.4 の対応）。トラッカーは、設定のニックネームではなく、VIVE Hub と同じ番号とシリアル番号の末尾で呼ぶ（3.2）。
- PR #37 のレビュー（1 回目）の指摘で直したもの: 全体の見え方を W-10 の 3 つにそろえたこと（確かめられないときは待機中の灰色。7.4 の「全体」）、VIVE Hub が止まったときに手引きの G1 を見張り役の具合より先にし、止まる前の接続・ホスト・電池・案内をいまの値として見せないこと（5.6、7.4 の「止まったときの見せ方」）、「フォルダーを選ぶ」を出すかを写しの値にしたこと（7.4 の `pick_folder`）、案内を出したあとにシリアル番号が分かった台を、同じ接続なら異常のまま見せること（3.6 と 6.2 の R1 の `conn`、7.4 の「案内の当て方」）。推奨の指摘と、PM が数えた U-18 の答え（VIVE Hub の番号は、つなぎ直しで振り直される）に合わせて、番号はつながっている台にだけ付け（3.2、5.1、5.4）、並べ方をシリアル番号の順にし、欄の案内の呼び名を枠の呼び名とそろえた（3.6 の `NoticeText`、7.4 の「並べ方」と「欄の文言」）。
- PR #37 のレビュー（2 回目）の指摘で直したもの: VIVE Hub が終了の行なしで止まったあいだと、起動し直して新しい開始の行を読むまでのあいだ（「止まっている・起動の途中」。5.6 の「出口の見張り」`HubGate`）は、通知の出口でも案内を出さず（7.1 の「VIVE Hub が止まっているあいだ」。`Engine::set_hub_down`。止まっているあいだに期限が来た案内と、待たせていた案内も出さない）、画面に止まる前の値を戻さない（7.4 の「全体」の 5「起動の途中」と、「止まったときの見せ方」）。プロセスの確かめは、これまでどおり検出の規則と集計には入れない（`Detection` と集計は、見張りと再生で同じ）。
- PR #37 のレビュー（3 回目）の指摘で直したもの: 開始待ちは、再構築をまたいで保ち、**止まった印**（止まった VIVE Hub が書いた記録の、最も新しい時刻）より後の時刻の開始の行でだけ下ろす（5.6。前の版〔レビュー 2 回目のあと〕の「追いつきの外の開始の行だけを数える」と「再構築では、確かめが『ある』なら下ろす」をやめた）。ドライバーのファイルだけが作り直されて再構築しても、新しい開始の行を読むまで、止まる前の値を画面に戻さず（7.4 の全体の 5 を、追いつきの 3 より先にした）、追いつきの終わりにも push しない（4.1、7.1）。アプリは、終了の行（`ServerStopped`）を渡した直後にも `set_hub_down` を渡し直し、`set_hub_down(true)` は、待たせている push に加えて、まだ `drain` で渡していない push も取り消す（3.7、7.1）。終了の行を読んだ回から push せず、その回の画面と通知が食い違わない。
- **ドライバーのファイルも読む**（4.1）。サーバーのファイルにない「電源オフの予告」（メモの 3.5）を、R4 の除外に使うためだけ。PRD の W-1 は、サーバーのファイルを挙げている（読むものを足すだけで、W-7 の「読むだけ」は変わらない。14 章で PM に確かめる）。
- M1 は実機に命令を送らない（W-7、D-13）。読むのはログのファイルだけ。書くのは、OpenUT 自身の保存場所のファイル、デスクトップの通知、SteamVR の中の表示だけ（11 章）。
- トラッカー内のログの収集（L-3）は、この文書の範囲に入れない。オーナーの PC のつなぎのスクリプトを確かめてから、別に依頼が出る（STATUS の「次の一手」の 6）。
- 画面と通知の**見た目**は UI/UX デザイン担当の範囲（`docs/design/ui/`）。この文書は、何をいつ見せるか（利用者に見せる状態、手引きの段、トラッカーの項目、読めないときの扱い）、案内の文言の意味、操作の流れまでを決める（7 章）。配置、印、色、書体、言い回しは決めない。
- 自分たちで測っていない数値は、そう書く。しきい値はすべて仮の値で、決め方を書く（6.1）。

## 1. 要件との対応

| 要件 | 設計の場所 | 実装計画 | 確かめ方 |
|---|---|---|---|
| W-1 ログを追記分だけ読み続ける（UTF-16、0 時の切り替わり） | 4.1〜4.7 | M1-1、M1-3、M1-4（直しは実装計画の 3 章の F-1〜F-3） | 単体テスト（解読、行の先頭、行の種類、追記の追跡。見本の行を入力にする）。オーナーの実機の確認（VIVE Hub が書いている最中に読めるか。U-13） |
| W-2 トラッカーごとの状態と、PC の Wi-Fi 情報 | 5 章 | M1-5 | 単体テスト（遷移の表の行ごと） |
| W-3 「Wi-Fi 情報なし」を検出し、電源を入れ直すトラッカーを案内する。全台オフやドングルのリセットを勧めない | 6.2 の R1、7.1 | M1-6 | 単体テスト、合成ログの再生テスト、受け入れの基準のテスト（M1-13） |
| W-4 地図同期が遅い、ホストが位置を出せない、操作と関係のない切断 | 6.2 の R2、R3、R4（R4 は、電源オフの予告と USB の抜き差しを除く） | M1-6 | 単体テスト、合成ログの再生テスト（ドライバーのファイルを含むケース） |
| W-5 SteamVR の中とデスクトップに出す。繰り返し出しすぎない | 7 章 | M1-7（配信の決まり）、M1-10、M1-11、M1-12 | 配信の決まりは単体テスト。表示はオーナーの実機の確認 |
| W-6 日ごとの集計をファイルに残す | 8 章 | M1-8、M1-9 | 単体テスト、再生テスト（期待する集計の値） |
| W-7 実機に命令を送らない | 0 章、11 章 | 全項目 | レビュー（M1 の依存に、HID・パイプ・ドングルを扱うものがないこと）。ログを読み取り専用で開くことの単体テスト |
| W-8 保存したログを時刻順に流す再生テスト | 10 章 | M1-2、M1-6、M1-13 | 再生テストが CI で動く |
| W-9 ログの置き場所、台数、シリアル番号を固定しない | 9 章、5.1 | M1-9 | 単体テスト（台数を変えた合成ログ、設定なしでの自動検出） |
| W-10 見せるのは、手引き、いまの状態（正常・待機中・異常）、異常のときにすること、だけ | 5.6（出口の見張り。止まっている・起動の途中）、7.1（文言と、使わない言葉。VIVE Hub が止まっているあいだは push しない）、7.4（見せるもの・見せないもの、利用者に見せる状態、全体〔3 つ。確かめられないときは待機中の灰色〕、止まったときの見せ方、手引き、仕組みの側の不具合の伝え方） | M1-7（表示の写しと文言、出口の見張り、止まっているあいだの配信）、M1-9（見張り役の具合、プロセスの確かめと `set_hub_down`）、M1-11 | 単体テスト（内部の状態と見張り役の具合から、利用者に見せる状態と手引きの段への写し。文言に使わない言葉が入らないこと。出口の見張りの表と、止まっているあいだに push しないこと）。見張りのループのテスト（終了の行のない停止、止まっているあいだに期限が来る案内、開始の行が遅れて届く再起動、起動の途中の再構築、終了の行を読んだ回）。見た目は UI/UX デザインとオーナーの確認 |
| W-11 トラッカーごとの項目（接続、電池、シリアル番号、状態、ファームウェアの版、ホスト）と、ドングルの接続 | 4.4（読む行）、5.1、5.4〜5.6、7.4 の「トラッカーの項目」 | M1-3、M1-5、M1-7、M1-11 | 単体テスト（見本の行から電池・版・番号・ドングルを読む。台帳とドングルの遷移。読めないときの値）。オーナーの確認（VIVE Hub の画面と見比べる。U-18〜U-20） |
| W-12 部位で呼ばない。VIVE Hub の番号とシリアル番号の末尾で示す | 3.2、5.4、6.2 の R1、7.1、7.4、9.1 | M1-5、M1-6、M1-7 | 単体テスト（呼び名と並べ方が、3 台・5 台、ホストの交代、スロットの使い回しで変わらないこと。番号が読めないときの呼び名） |
| 受け入れ: 2026-11-22〜27 のログで W-3 の検出が 4 回だけ | 6.2 の R1、10.4 | M1-13 | 再生テスト（匿名化した実物のログ。PM が `tests/` に入れたあと） |
| 受け入れ: VR の動作を重くしない | 12 章 | M1-9〜M1-12 | オーナーの実機の確認（12 章の手順） |
| 受け入れ: 想定と違う行でも止まらずに読み続ける | 4.7 | M1-1、M1-3 | 単体テスト（乱れたバイト列、知らない行、壊れた数値） |

## 2. 部品と流れ

```
 VIVE Hub のログのフォルダー（サーバーとドライバーのファイル）
        │ （読み取り専用。500 ms ごとに大きさを見る）
        ▼
 ┌─ openut（アプリ）───────────────────────────────────────────┐
 │  追記の読み取り（ファイルの I/O）                           │
 │        │ バイト列                                           │
 │        ▼                                                    │
 │  ┌─ openut-hublog（入出力なし）──────────────┐              │
 │  │ 追記の追跡の判断 → 解読（UTF-16）→ 記録   │              │
 │  │ → 先頭の解析 → ずれの補い → 時刻順 → 種類 │              │
 │  └───────────────────────────────────────────┘              │
 │        │ 時刻つきの出来事                                   │
 │        ▼                                                    │
 │  ┌─ openut-core（入出力なし）────────────────┐              │
 │  │ エンジン: 状態 → 検出の規則 → 案内の配信  │              │
 │  │           └→ 日ごとの集計                 │              │
 │  └───────────────────────────────────────────┘              │
 │        │ 案内、状態の写し、集計                             │
 │        ▼                                                    │
 │  通知の出口: コンソール / デスクトップ / SteamVR の中        │
 │  トレイと状態の窓、集計のファイル                           │
 └─────────────────────────────────────────────────────────────┘
```

スレッド（M1-9 以降のアプリ）:

| スレッド | すること |
|---|---|
| 主スレッド | トレイと状態の窓（イベントループ）。画面なしの `openut watch` では使わない |
| 見張り | 500 ms ごとに追記を読み、エンジンに渡し、出力を配る。集計を 60 秒ごとと日の終わりに書く。Windows では、起動したときと 10 秒ごとに VIVE Hub のプロセスがあるかを確かめ、出口の見張り（5.6 の `HubGate`）を進めて、エンジンの配信に `set_hub_down` を渡す（7.1）。見張り役の具合（7.4）を決め、表示の写し（7.4）を作って主スレッドに渡す |
| SteamVR | SteamVR につながるのを 10 秒ごとに試し、つながっていれば案内を VR の中に出す |

スレッドのあいだは `std::sync::mpsc` でつなぐ。非同期のランタイム（tokio など）は使わない（入出力が少なく、要らないため）。

## 3. 部品のあいだの型

識別子は英語。ここに書く型の名前とフィールドは、実装の目安（意味を変えない範囲で、実装担当が整えてよい。変えたら PR の説明に書く）。

### 3.1 時刻（`openut_core::time`）

```rust
/// An instant, in milliseconds since the Unix epoch (UTC).
pub struct LogTime(pub i64);
/// Minutes east of UTC, as written in the log header (e.g. "+9" -> 540).
pub struct UtcOffset(pub i16);
/// A local calendar date (the date of a log file, the day of a summary).
pub struct LogDate { pub year: i32, pub month: u8, pub day: u8 }
```

- ログの行の時刻は、地方時と UTC からのずれ（行の先頭の `+9`。U-3）で書かれている。`LogTime` は、それを UTC に直したもの。日付の計算（暦の日付と通し日数の変換）は自前で書く（短い計算なので、時刻のクレートを足さない）。
- ずれの読めない行は、行ごとに 0 とせず、前の行のずれを引き継いで UTC に直す（4.3 の「ずれの補い」）。ずれのある行のあいだに 0 の行が 1 つ混ざると、そこだけ時刻が 9 時間先へ飛び、次の行で戻るため。**サーバーとドライバーの 2 つの列（ファイルの記録の並び）で、時間の基準を 1 つにする**（基準のずれは、サーバーの列の最初の記録で決め、ドライバーの列もそれに従う。4.3）。列ごとに決めると、片方の先頭だけずれが欠けたときに、同じ地方時の 2 行が 9 時間離れて比べられるため。行の `offset`（`Timed` の `offset`）は、補ったあとの値で、地方の日付（8.1）もこの値で求める。
- 規則と集計は、**ログの時刻だけ**を使う。壁時計は、見張りのループが「時刻の進め方」（4.6）のために使うだけで、エンジンには `LogTime` として渡す。
- 時間の長さはミリ秒の `i64` で持つ。

### 3.2 トラッカーの呼び名（`openut_core::tracker`）

```rust
/// The slot the source of events uses for a tracker while it is connected.
/// M1: the dongle slot N (0..=4) in "(N)" / "(index: N)" / "device index: N" in the VIVE Hub log.
/// M3: the slot number of the runtime.
/// Not stable across reconnections: never use it to name a tracker, never show it to the user.
pub struct TrackerKey(pub u16);
/// A serial number, when the source tells it. Never written to the repository.
pub struct Serial(pub String);
/// The number VIVE Hub shows for a tracker: N of "model name: (VIVE Ultimate Tracker N)" (4.4).
pub struct HubNumber(pub u16);
/// How a tracker is called on the screen and in the notices (W-12). Never built from the slot.
pub struct TrackerLabel {
    pub hub_number: Option<HubNumber>, // of the current connection only (below). None when disconnected
    pub serial_tail: Option<String>,   // the last characters of the serial (length: below)
    pub host: bool,                    // used only when both are None ("ホストのトラッカー")
}
```

- **番号 N はドングルのスロット**（0〜4）で、つなぎ直しで変わる。同じトラッカーが、同じ日に別の番号で出る（メモの 3.5）。**利用者には見せない**（VIVE Hub の画面の番号とは別の数のため。W-12）。
- 状態と規則は、出来事が持つ番号（`TrackerKey`）をキーにした表で動く（どの出来事にも番号が入り、シリアル番号が入るのは 1 種類の行だけのため）。台数を決めない（W-9）。初めて出てきた番号で行を足す。
- **トラッカーそのものは、シリアル番号で見分ける**。番号とシリアル番号の対応は `SerialKnown`（4.4 の `GetDeviceInfoAndResponse` の行。接続のたびに何度も出る）で分かり、その番号の行に付ける（5.1）。VIVE Hub の番号も、同じ行で分かり、その番号の行（いまの接続）に付ける（5.1）。シリアル番号ごとに分かったこと（ファームウェアの版、最後に見た時刻）は、台帳（5.4）に持つ。利用者に見せる呼び名と、日ごとの集計の行は、シリアル番号で引く（7.4、8 章）。

**呼び名**（W-12。`openut_core::text`）: 利用者に見せる呼び名は、**VIVE Hub と同じ番号**（`model name: (VIVE Ultimate Tracker N)` の N。4.4）と、**シリアル番号の末尾**の 2 つで作る（PRD の W-12。示し方は PM の判断。#27）。

- **呼び名にしないもの**: 体の部位（腰、足、胸）、スロットの番号 N、ホストかどうか、役割。部位はトラッカーに決まっておらず（使い方で替わる）、スロットは VIVE Hub の画面の番号と違う数で、ホストは使っているうちに替わるため。初版の設定のニックネーム（9.1 の `[trackers."<シリアル番号>"] name = "腰"`）と、名前を付ける流れ（7.4 の F3）は、やめた（部位の名前を付ける前提の作りだったため。呼び名は、ログから決まるものだけにする）。
- **VIVE Hub の番号**は、その台の**いまの接続**で最後に読めた `SerialKnown` の値（5.1 の `hub_number`）。**台ごとに固定ではない**: つなぎ直すたびに振り直されているように見える（U-18。PM が 2026-11-23〜28 のログで数えた。3 台とも、6 日のうちに 1・2・3 のすべてになり、同じ晩のうちにも変わった）。そのため、**つながっていない台には番号を付けない**（末尾だけで呼ぶ）。切れる前の番号を残すと、つなぎ直した別の台が同じ番号になり、同じ番号の枠が 2 つ並ぶため。番号は「VIVE Hub の画面で、いまどれか」を示す手がかりで、台を見分けるのは末尾（シリアル番号）。
- **末尾の長さ**: 台帳（5.4）にあるすべてのシリアル番号について、末尾が重ならない最も短い長さ。4 文字から始め、重なれば 1 文字ずつ延ばす（最長はシリアル番号の全体。全体が同じなら同じ台）。台帳の全台に同じ長さを使う（並べたときにそろうように）。台帳が変わったら決め直す。シリアル番号が 4 文字より短ければ、全体を使う。
  - 長さが変わるのは、末尾の重なる台が台帳に入ったとき（と、台帳から消えたとき。5.4 の 17 台目、読み直し）だけ。そのときは、枠の呼び名と、状態の窓の欄の案内の呼び名が、同じ写しの中で一緒に変わる（欄の文言は、写しを作るたびに、いまの呼び名で作る。3.6、7.4）。例は 7.4 の「並べ方」。
- **読めないとき**（シリアル番号は `GetDeviceInfoAndResponse` の行で来るので、番号だけ分かることはない）:

  | 分かっていること | 呼び名（意味） | 文言の既定（仮。言い回しは UI/UX デザイン担当） |
  |---|---|---|
  | 番号と末尾（つながっている台） | 番号と末尾 | `トラッカー 1（…0001）` |
  | 末尾だけ（つながっていない台。つながっていて、`model name` が読めない台） | 末尾だけ | `トラッカー（…0001）` |
  | どちらもない（シリアル番号がまだ来ていない。途中から読み始めて、接続の始まりを見ていない台） | ホストの接続が続いていれば「ホストのトラッカー」、でなければ「トラッカー」 | `ホストのトラッカー`、`トラッカー` |

  - シリアル番号は、ふつう接続から 1 秒以内に来る（見本の 4 章）ので、どちらもないのは、接続の直後と、前の日のファイルのない途中からの読み始めのときだけ。画面では、どの枠か（7.4 のトラッカーの項目）と、ホストの印で見分けられる。
- **呼び名の末尾は、台数、ホストの交代、スロットの番号に左右されない**（3 台でも 5 台でも、胸などに足しても、同じ形）。番号の部分は、VIVE Hub がつなぎ直しで振り直すのに合わせて変わる（上）。呼び名を決めるのは、案内を作るとき（3.6）と、表示の写しを作るとき（7.4）で、そのときの台帳とスロットの行から作る。**push した案内の文言**（VR の中、デスクトップ）は、出したときの呼び名のまま変えない。**状態の窓の欄の案内**は、写しを作るたびに、その台のいまの呼び名で作る（枠の呼び名とそろえるため。7.4）。
- 番号で名前を引くことはしない（つなぎ直すと、別のトラッカーを指すため）。

### 3.3 ログの 1 行（`openut_hublog`）

```rust
/// A record split into its header and body (4.3).
pub enum ParsedLine<'a> {
    Entry(Entry<'a>),
    /// A line starting with '#': written when the process starts or the file rotates (4.3).
    Meta(&'a str),
    /// No header could be read: a broken record.
    NoHeader(&'a str),
}
pub struct Entry<'a> {
    pub at: LogTime,
    pub offset: UtcOffset,
    /// false when the offset could not be read. The header parser then uses 0;
    /// OffsetFill (4.3. One for both files) replaces at and offset before the entry is used.
    pub offset_known: bool,
    pub level: &'a str,
    pub func: &'a str,
    pub thread: Option<&'a str>,
    pub body: &'a str,
}
/// Which file a record came from (4.1). Driver records never move the engine time (3.7).
pub enum FileKind { Server, Driver }
/// What a body says (4.4).
pub enum LineKind { ServerStart, ServerRelease, DongleAdded, DongleRemoved, TrackerAdded, TrackerRemoved,
    DeviceInfo, HostMode, HostWifiQuery, HostWifiGot, WifiAssign, WifiNotYet, ClientLink,
    DeviceStatus, ServiceStatus, Pose, OetLost, OetReloc, OetOverspeed, Command, PowerOffNotice, Other }
```

- `openut_hublog::record` は、`Entry` から、種類と読み取った値（4.4 の表の「取り出すもの」）を返す。値の持ち方（種類ごとの列挙か、共通の構造体か）は実装担当が決めてよい。
- 行の種類が分かったのに、番号や値が読めなかったときは、出来事を作らず、「読めなかった行」として数える（4.7）。

### 3.4 出来事（`openut_core::event`）

状態・規則・集計の入力。**ログの行の形に依存しない**（R-9。M3 では、ランタイムが同じ出来事を出す）。

```rust
pub struct Timed { pub at: LogTime, pub offset: UtcOffset, pub event: Event }
pub enum Event {
    ServerStarted,                                            // the PC-side server started
    ServerStopped,                                            // the server began to shut down
    DongleAdded,                                              // the dongle was found (plugged in, or at server start)
    DongleRemoved,                                            // every tracker connected through the dongle is gone
    DongleReport { connected: bool },                         // a report of the dongle, not a change (5.5)
    TrackerAdded { tracker: TrackerKey, usb: bool },
    TrackerRemoved { tracker: TrackerKey, usb: bool },        // usb: connected by cable, not by the dongle
    SerialKnown { tracker: TrackerKey, serial: Serial,
                  hub_number: Option<HubNumber>,              // N of "VIVE Ultimate Tracker N", if readable
                  firmware: Option<String> },                 // "fw version: (...)", if readable
    HostAssigned { tracker: TrackerKey },                     // PC told the tracker to be the host
    HostWifiQueried { host: TrackerKey },                     // PC asked the host for its Wi-Fi info
    HostWifiObtained { host: Option<TrackerKey> },            // PC got the host's Wi-Fi info (M1: None)
    WifiRequestUnanswered { tracker: Option<TrackerKey> },    // a client asked, PC had nothing (M1: None)
    WifiAssigned { tracker: TrackerKey },                     // PC gave the info to a client
    ClientLink { tracker: TrackerKey, connected: bool },      // client joined / left the host's Wi-Fi
    Status { tracker: TrackerKey, connected: bool, host: bool, map_ready: bool,
             usb: Option<bool>,                               // a report, not a change. usb: None if not written
             battery: Option<Battery> },                      // None: not connected, or not readable (4.4)
    Pose { tracker: TrackerKey, status: PoseStatus },
    Oet { tracker: TrackerKey, kind: OetKind },
    PowerOffNotice { tracker: TrackerKey },                   // the tracker said it is powering off
    PcCommand { tracker: Option<TrackerKey>, kind: String },  // other PC -> tracker commands. None: all
}
pub enum PoseStatus { Ok, Lost, RecentlyLost, Other(i64) }    // 2, 3, 4
pub enum OetKind { LostTracking, RelocBack, Overspeed }
/// The battery of a connected tracker (W-11).
pub struct Battery { pub percent: u8 /* 1..=100 */, pub charging: Option<bool> }
```

- `HostAssigned`、`HostWifiQueried`、`WifiAssigned` は、PC からトラッカーへの命令でもある。R4 の「直前に命令があったか」は、この 3 つと `PcCommand` を見る。`PcCommand` の `tracker: None` は、番号のない命令（`Unpair All` など）で、すべてのトラッカーへの命令として扱う。
- `Status` は、トラッカーごとの状態の報告（4.4 の `DeviceStatusToVHServer` の行）。変わったときだけでなく、数分おきにも出るので、状態を変える出来事ではなく、いまの様子の報告として扱う（5.1）。`host` はホストの根拠（5.3）、`usb` は接続の経路（5.1。`TrackerAdded` を見ていない台のため）に使う。
- `TrackerAdded` と `TrackerRemoved` の `usb` は、`connection type: 2`（USB でつないだ。推定。メモの 3.5）のとき `true`。ドングルを通さない接続なので、`DongleRemoved` では切れない（5.1）。
- `HostWifiObtained` と `WifiRequestUnanswered` の行には、トラッカーの番号がない（メモの 3.1）。M1 では常に `None` で、5 章の「番号のない出来事」の扱いになる（M3 のランタイムは番号を持てるので、型には残す）。
- 初版の `Map` の出来事は、`Status` の `map_ready` に置き換えた（地図の状態は単独の行ではなく、状態の行の一部のため。メモの 3.1）。
- **画面に見せる項目のための値**（W-11。#27 で足した）: `SerialKnown` の `hub_number` と `firmware`、`Status` の `battery`、`DongleAdded` と `DongleReport`。どれも、台帳（5.4）、スロットの行の VIVE Hub の番号と電池（5.1）、ドングルの表示（5.5）だけに使い、規則（6 章）と、トラッカーの状態の遷移（5.1 の `phase`）は変えない。`hub_number` か `firmware` が読めなくても、`SerialKnown` は出す（シリアル番号だけで台を見分けられるため。4.4）。

### 3.5 状態の写し（`openut_core::state`）

```rust
pub enum Phase { Disconnected, Connected, WaitingWifi, MapSyncing, Relocalizing, Tracking }
pub enum Role { Unknown, Host, Client }
/// How the tracker is connected to the PC (5.1).
pub enum Link { Dongle, Usb, Unknown }
/// One connection of a slot, from connected to disconnected (5.1). Numbered from 1 in the
/// order the connections start; never reused until rebuild.
pub struct ConnId(pub u64);
pub struct TrackerView {
    pub key: TrackerKey, pub serial: Option<Serial>, pub role: Role,
    pub phase: Phase, pub since: LogTime,
    pub conn: ConnId,                  // the current connection, or the last one when Disconnected
    pub link: Link,                    // from TrackerAdded (usb), or from Status (usb) when inferred
    pub tracked_since_connect: bool,   // reached Tracking at least once since TrackerAdded
    pub inferred: bool,                // first seen without TrackerAdded (started mid-way)
    pub battery: Option<Battery>,      // the last report of the current connection (5.1). None when disconnected
    pub hub_number: Option<HubNumber>, // from SerialKnown of the current connection (3.2, 5.1). None when disconnected
}
/// What tells which tracker is the host (5.3). Kept apart from HostWifi.
pub enum HostBy { Command, Report }    // Command: HostAssigned. Report: a status report or a Wi-Fi query
pub struct HostRef {
    pub key: TrackerKey, pub conn: ConnId,   // the connection the evidence was about
    pub serial: Option<Serial>,              // the serial of that connection, when known
    pub by: HostBy, pub since: LogTime,
}
pub enum HostWifi {
    Unknown,                                    // no evidence yet
    Missing { since: LogTime, queried: bool },
    Held { since: LogTime },
}
/// What the log told about one tracker, by serial (5.4). Kept after it disconnects.
/// No VIVE Hub number: VIVE Hub gives it again at each connection (3.2, U-18).
pub struct Identity {
    pub serial: Serial, pub firmware: Option<String>,
    pub last_seen: LogTime,
}
/// The dongle as the log tells it (5.5).
pub enum DongleState { Unknown, Connected, Disconnected }
/// The VIVE Hub tracker server as the log tells it (5.6).
pub enum HubState { Unknown, Running, Stopped }
/// How a tracker is connected, as shown to the user (7.4).
pub enum Connection { Disconnected, Dongle, Usb }
/// One tracker as the screen lists it (5.4): one per serial in the roster, plus one per
/// connected slot whose serial is not known yet (serial: None).
pub struct DeviceView {
    pub serial: Option<Serial>, pub firmware: Option<String>,
    pub hub_number: Option<HubNumber>, // of the current connection (3.2). None when disconnected
    pub key: Option<TrackerKey>,       // the slot while connected. Internal: never shown
    pub conn: Option<ConnId>,          // the current connection. Internal: matches notices (7.4)
    pub connection: Connection,
    pub battery: Option<Battery>,
    pub host: bool,                    // the host connection is going on (below)
    pub phase: Phase,                  // internal: the display maps it to 正常・待機中 (7.4)
}
pub struct Snapshot {
    pub trackers: Vec<TrackerView>, pub host: Option<HostRef>, pub host_wifi: HostWifi,
    pub roster: Vec<Identity>, pub devices: Vec<DeviceView>,
    pub dongle: DongleState, pub hub: HubState, pub at: LogTime,
}
```

- **ホスト**（`host`）と **PC の Wi-Fi 情報**（`host_wifi`）は別に持つ。途中から読み始めると、Wi-Fi 情報は `Unknown` のままでも、状態の行の `is host: 1` でホストは分かるため（5.3）。R1 の案内するトラッカー（6.2）は `host` を使い、番号のない問い合わせの候補（5.1）は、`host` から決まる役割を使う。
- **ホストの根拠は、番号だけでなく接続（`conn`）に結びつける**（5.3）。番号はスロットで、ホストが切れたあとに別のトラッカーが同じ番号でつながりうるため。「ホストの接続が続いている」は、番号 `key` の行が未接続でなく、その行の `conn` が `HostRef::conn` と同じこと。続いていなければ、その番号の台をホストとして案内しない（6.2 の R1）。トラッカーの項目のホストの印（7.4。`DeviceView::host`）も、接続が続いているときだけ付ける。

**内部の状態の名前**（W-2）: `Disconnected` 未接続、`Connected` 接続、`WaitingWifi` Wi-Fi 待ち、`MapSyncing` 地図同期中、`Relocalizing` 位置取り直し中、`Tracking` 追跡中。PC の Wi-Fi 情報: `Held` 持っている、`Missing` 持っていない、`Unknown` 不明。**これらの名前は、画面と通知に出さない**（W-10）。使うのは、検出の規則、アプリのログ、テストの表示だけ。利用者には、7.4 の「利用者に見せる状態」（正常・待機中・異常）に写して見せる。

### 3.6 検出と案内（`openut_core::rules`、`openut_core::notice`）

```rust
pub enum RuleId { R1, R2, R3, R4, R5 }
/// One run of a rule's condition, from start to end.
pub struct Detection {
    pub rule: RuleId, pub tracker: Option<TrackerKey>, pub episode: u64,
    pub serial: Option<Serial>,   // the tracker the episode is about, when known (below)
    pub conn: Option<ConnId>,     // the connection the episode is about, when known (below)
    pub at: LogTime, pub step: Step,
}
pub enum Step { Raised, Escalated, Resolved, Ended }   // Ended: the condition went away without a fix
pub struct Notice {
    pub rule: RuleId, pub tracker: Option<TrackerKey>, pub episode: u64,
    pub serial: Option<Serial>, pub conn: Option<ConnId>,
    pub step: Step, pub at: LogTime,
    /// The words (7.1) with the slot for the tracker it is about ({T}; {H} for R1) left open.
    /// Push fills the slot with `target`; the status window fills it with the label of now (7.4).
    pub text: NoticeText,
    pub target: TrackerLabel,   // the label of {T} ({H} for R1) when the notice was made (3.2)
    pub push: bool,         // true: show in VR and on the desktop. false: status window only.
    pub priority: u8,       // R1 = 5 (highest) ... R5 = 1
}
/// Headline: the state, one short line. Action: what to do now, one line (empty for Resolved).
/// Japanese. Other labels in the words (the {H} of R2) are filled when the notice is made.
pub struct NoticeText { pub headline: Vec<TextPart>, pub action: Vec<TextPart> }
pub enum TextPart { Words(String), Target }
```

- 規則は `Detection` を出す。配信の決まり（7.1）が、`Detection` から `Notice` を作り、`push` を決める。どちらも入出力を持たず、単体テストで確かめる。
- **`serial`**（#27 で足した）: エピソードが指す台のシリアル番号（分かれば）。R1 は案内する台（6.2 の R1 の「案内するトラッカー」の 1 か 2 で決まった台。3 なら `None`）、R2・R3・R5 は `Raised` のときの番号 N の行のシリアル番号、R4 は切れた接続のシリアル番号（切れる前に分かっていたもの）。表示の写し（7.4）が、どの台を「異常」にするかを、スロットではなく台で決めるため（R4 の台は、切れたあとスロットの行からシリアル番号が外れる。5.1）と、同じ台の繰り返し（7.1）を台で数えるために使う。
- **`conn`**（PR #37 のレビューの指摘で足した）: エピソードが指す接続（5.1 の `ConnId`）。R1 は、案内する台が 1 で決まればホストの接続（`HostRef::conn`）、2 でいま番号 M でつながっていれば M のいまの接続、2 でつながっていないときと 3 では `None`。R2・R3・R5 は `Raised` のときの番号 N のいまの接続。R4 は切れた接続。表示の写し（7.4）が、**案内を出したときにシリアル番号がまだ分からなかった台**を、同じ接続のあいだに `SerialKnown` が来て台帳の台になっても、同じ台として「異常」に見せるために使う。つなぎ直して同じスロットを別の台が使えば、接続が別なので当たらない（7.4 の「利用者に見せる状態」）。`ConnId` はエンジンの中の通し番号で、`rebuild` で振り直されるので、再構築をまたいだ照合（7.1）には使わない。
- **いつ決めるか**: `tracker`、`serial`、`conn` は、`Raised` のときに決め、あとに分かっても変えない（`Resolved`、`Ended` は、そのエピソードの最後の値を引き継ぐ）。ただし R1 は、案内する台を `Escalated` でも決め直す（6.2 の R1。案内を作るたびに決めるため）ので、`Escalated` のときにも、3 つをその台の値にする。
- 文言（7.1 の表）は `openut_core::text` にまとめ、トラッカーの呼び名（3.2）を差し込む関数にする（テストで文言を確かめられるように）。`NoticeText` の穴（`Target`）に呼び名を入れて文字列にする関数も、`text` に置く。push の出口（7.2、7.3、コンソール）は `target` で、状態の窓の欄（7.4）はいまの呼び名で、同じ関数を通す。R4 の「回数を足す」（7.1）と、R5 の秒数（7.1）は、`Words` の中に入れる。

### 3.7 エンジン（`openut_core::engine`）

```rust
impl Engine {
    pub fn new(cfg: EngineConfig) -> Engine;   // no names table: labels come from the roster (3.2)
    /// The caller promises that every later input from the server file has at >= t.
    /// Fire every timer whose deadline is < t (in deadline order), then set now = t.
    /// A timer whose deadline == t stays pending: an input at the deadline is in time (6.1).
    pub fn advance_to(&mut self, t: LogTime);
    /// advance_to(ev.at), then update the state, the rules and the summary.
    /// Events of the driver file (PowerOffNotice) never move the time: they are kept at ev.at (4.6).
    pub fn apply(&mut self, ev: Timed);
    /// Other / Unparsed / NoHeader. With Some(at), for the server file: advance_to(at) and mark
    /// the line as seen (8.1). Lines of the driver file are only counted (on the day of at).
    pub fn note_line(&mut self, file: FileKind, at: Option<LogTime>, quality: LineQuality);
    /// The engine must not be moved past this time (by apply of a server event, note_line of a
    /// server line, or advance_to) until every driver record with at <= this time has been given.
    /// The earliest of: the deadline of a pending R4 decision, the last instant of the open day (4.6).
    pub fn driver_needed_through(&self) -> Option<LogTime>;
    pub fn set_catching_up(&mut self, on: bool);                            // 4.5
    /// VIVE Hub is stopped or starting again (5.6, HubGate). Push only (7.1): detections,
    /// the state and the summaries do not change. On true, the pushes that wait (7.1) and the
    /// pushes not drained yet are taken back: they become push = false, as never pushed.
    pub fn set_hub_down(&mut self, on: bool);
    /// Start over from the initial state (4.1, 4.5): drop the state, the rules, the summaries
    /// and now. Keep what the notice rules remember (7.1).
    pub fn rebuild(&mut self);
    pub fn next_deadline(&self) -> Option<LogTime>;
    pub fn drain(&mut self) -> Vec<Output>;   // Detection, Notice, DayClosed(Summary)
    pub fn snapshot(&self) -> Snapshot;
    pub fn open_summaries(&self) -> Vec<Summary>;  // days not closed yet (8.1)
}
```

- `EngineConfig` は、しきい値（6 章の表の既定値）と配信の決まりの値を持つ。設定ファイル（9.1）と再生テスト（10.2）から差し替えられる。
- **同じ時刻の入力と期限の順**（見張りと再生で、同じ検出が同じ `at` で出るようにする）:
  - 同じ時刻の入力は、渡された順（ファイルの行の順）に適用する。
  - 期限の判定は、時刻が期限を**過ぎてから**行う（`advance_to(t)` で、期限 < t のもの）。期限と同じ時刻の入力は、期限の判定より先に適用されるので、期限に間に合ったものとして扱われる（6.1）。判定した `Detection` の `at` は、期限の時刻。
  - 同じ期限のものは、規則の順（R1〜R5）、トラッカーの番号の順、期限を作った順に判定する。
  - 見張りのループは 500 ms ごとに `advance_to(見張りの時刻)` を挟み（4.6）、再生は入力の時刻だけで進む（10.2）。どちらでも、期限 D の判定は「D より後の時刻に進んだとき」で、D ちょうどの入力が先に入るので、結果は同じになる。見張りは、ドライバーの記録を待つ時刻（`driver_needed_through`）より先には、ドライバーを読み終えるまで進めない（4.6 の「ドライバーを待つ」）。違いうるのは、見張りで L より遅れて届いた行（サーバーの時刻が戻る行と、ドライバーの遅れた記録。4.6）があったときだけ。
  - `next_deadline()` は、まだ判定していない最も早い期限（その時刻を過ぎると判定される）。
- エンジンの時刻を進めるのは、**サーバーのファイルの記録と `advance_to` だけ**。ドライバーのファイルの記録（`PowerOffNotice` と、数えるだけの行）は、時刻を進めず、状態も変えない。`PowerOffNotice` は、R4 が振り返って探すために元の時刻のまま覚え、数えるだけの行は、その行の時刻の日に数える（まだ始まっていない日の分は、その日が始まるまで持っておく）。ドライバーのファイルは、サーバーのファイルと別にまとめて書かれ、遅れても進んでも届きうるので、届いた順で時刻を動かさないため（4.6）。
- サーバーの記録で、時刻が戻るもの（`ev.at` < いまの時刻）は、いまの時刻に起きたものとして扱い、数える（4.6）。
- `driver_needed_through()` は、R4 の判定の期限のうちまだ判定していない最も早いもの（条件 5 を確かめるとき。`rules.r4.exclude_power_off = true`）と、いま数えている日の最後の時刻（次の日の 0 時の 1 ms 前。ドライバーの行の数を、日を閉じる前にそろえるため。8.1）の、早いほう。再生は、時刻の順に合わせて流すので、これを見なくても同じ順になる（10.2）。
- `rebuild` は、起動し直したのと同じ状態に戻す（作り直しと、日が飛んだとき。4.1）。配信の決まりが覚えていること（push 済みで続いているエピソード、同じ案内を繰り返さない決まりの時刻、push の数）だけは残す。
- `set_hub_down` は、アプリ（M1-9）が、出口の見張り（5.6）の答えを渡すもの。配信の決まり（7.1 の「VIVE Hub が止まっているあいだ」）だけが使い、規則・状態・集計には渡さない（D-22。プロセスの確かめは、ログに書かれていないことのため）。アプリは、プロセスを確かめた回と、`HubState` を変える出来事（`ServerStarted`、`ServerStopped`）を渡した直後（次の出来事を渡す前）と、`rebuild` のあとに渡し直す（M1-9 の順）。`rebuild` では変えない（アプリが渡し直す）。`true` を受けたら、待たせている push と、まだ `drain` で渡していない push の `Notice` を、`push = false` にする（取り消す。7.1）。`drain` が出口への区切りなので、同じ回の写し（7.4）が止まった・起動の途中の見せ方なら、その回に出口へ渡る push はない。再生（10.2）と `openut replay` は呼ばない。

## 4. ログの読み取り（W-1）

### 4.1 ファイルの選び方と 0 時の切り替わり

- 対象は、ログのフォルダー（9.2）の直下の、次の 2 種類（大文字と小文字を区別しない）。種類ごとに、名前の日付が最も新しいものを「いまのファイル」にする。名前が形に合わないファイルは無視する。
  - **サーバー**: `ViveTrackerServer_YYYYMMDD.txt`。M1 の出来事のほとんどは、ここから取る（4.4）。これがないフォルダーは、ログのフォルダーとみなさない（9.2）。
  - **ドライバー**: `ViveTrackerDriver_YYYYMMDD.txt`。電源オフの予告（4.4 の `PowerOffNotice`）だけを取り、ほかの行は数えるだけ。なくても動く（R4 で、電源オフの予告を除けなくなるだけ。6.2）。
  - 同じフォルダーのドングルのファイル（`ViveTrackerDongle_*.txt`）とほかのファイルは読まない（M1 の規則に使う行がなく、無線のアドレスが入るため。メモの 3.4、4 章）。
- 読み取りの手順（世代、作り直し、追いつき）は、2 種類のそれぞれに同じものを当てる（`Tailer` を種類ごとに 1 つ持つ）。2 種類の行を時刻の順に並べて出来事にする決まりは 4.6。
- 500 ms ごとに、フォルダーの一覧と、いまのファイルの大きさを見る（OS のファイル変更の通知は使わない。追記の検出だけなら定期の確認で足り、通知の取りこぼしを考えなくてよいため）。
- **0 時の切り替わり**（メモの 2.5）: VIVE Hub は、日付が変わったあとの最初の書き込みのときに切り替える（0 時ちょうどではない。サーバーで 00:00〜00:02、ドライバーでは 01:41 のこともある）。古いファイルの最後に、次の日の日付の記録が 1 件入り、新しいファイルは `# Log rotate by date at ...` で始まる。そのため、次の決まりにする。
  - いまのファイルより新しい日付のファイルが現れたら、いまのファイルを最後まで読み、記録の途中で終わっていた残りを 1 件として流してから、新しいファイルの先頭に移る。古いファイルにある次の日の時刻の記録は、行の時刻で次の日に数える（8.1）。
  - 移ったあと 30 秒のあいだに古いファイルが伸びたら、その分は読まずに「切り替わったあとの書き足し」として数え、アプリのログに残す。6 日分では起きていない（切り替わりのあとの記録は、どれも新しいファイルに入っていた）。起きたら、VIVE Hub の動きが変わった印として扱う。
  - 状態と集計は、そのまま新しいファイルに持ち越す（日の区切りは行の時刻で決める。8.1）。ただし、新しいファイルの日付が、いまのファイルの日付の次の日でなければ（日が飛んだ）、閉じていない日の集計をそのまま（`final: false`）書いてから、`Engine::rebuild` で初期の状態に戻して移る（読み直すものはない）。起動し直したときに前の日のファイルがない場合（4.5）と同じにするため。ドライバーの日が飛んだだけのときは、`rebuild` しない（ドライバーからは状態を作らないため）。
  - 大きさが `MaxFileSize`（15 MB）を超えたときの動きは分からない（メモの 2.5。最も大きいファイルで 5.5 MB）。名前の末尾に `_N` の付いたファイル（`ViveTrackerServer_YYYYMMDD_1.txt` など）が現れたら、読まずに、アプリのログに 1 回だけ残す。
- **世代**: ファイルを先頭から読み始めるたびに、新しい世代の番号を付ける（起動、0 時の切り替わり、作り直し）。読んだバイト列には、ファイル名と世代を付けて渡す。解読（4.2）の途中の状態は、世代ごとに持つ。
- **作り直しの見分け方**: 次のどれかが起きたら、いまのファイルが作り直されたとみなす。
  1. 縮んだ（大きさが読んだ位置より小さい）。
  2. 伸びたときに、先頭の部分が前と違う（置き換えられた）。先頭の部分は、その世代で最初に読んだ 4 KiB（読んだのがそれより短ければ、読んだ分）。伸びたのを見つけるたびに、この分だけ読み直して比べる。
  3. いまのファイルが消えた。
  - 大きさが同じままの置き換えは、伸びるまで気づかない（仕方ないものとする）。先頭の 4 KiB には、その世代の最初の行の時刻が入るので、別の中身で先頭が一致することは考えない。
- **作り直したときの再構築**: 起動し直したときと同じ手順を、アプリを止めずに行う。これで、前の世代で読んだ行が新しいファイルにもあっても、回数を二重に数えず、古い行を今の出来事として案内しない。
  1. その時点の、閉じていない日の集計を、作り直しの前の分として別のファイルに書く（9.3。あとの集計と足し合わせない）。新しいファイルに前の世代の行が残っていないとき（切り詰めて書き直されたとき）に、消えた分を失わないため。追いつきの途中で作り直しが起きたときは、書かない（まだ組み立て途中の値のため）。
  2. `Engine::rebuild` で、状態、規則、集計を捨てる。配信の決まりが覚えていることは残す（3.7）。
  3. 4.5 の手順（前の日のファイル、いまのファイルの先頭から）で、追いつきとして読む。追いつきのあいだは、集計を書き出さない。
  4. 追いつきが終わったときの配信は 4.5 と 7.1 のとおり。作り直しの前に push 済みで続いていたエピソードと、規則とトラッカーが同じで続いているエピソードは、同じものの続きとして push し直さない（7.1）。
  - 見分け方の 3（消えた）のときは、残っているうちで最も新しいものを、いまのファイルにして再構築する。1 つもなければ、見張り役の具合を「見つからない」（7.4 の「仕組みの側の不具合」）にして、見つかったら再構築する。
  - 再構築したあとの集計は、起動し直した場合と同じ（いまあるファイルから決まる。8.1）。再構築の回数を `source.rebuilds` に数え、アプリのログに理由（縮んだ、置き換えられた、消えた）を残す。
  - サーバーとドライバーの、どちらのいまのファイルが作り直されても、両方を 4.5 の手順で読み直す（決まりを 1 つにするため。6 日分では、作り直しは起きていない。メモの 2.3 の `Truncate=0`）。ドライバーのファイルが消えて 1 つも残らないときは、再構築せず、ドライバーなしで続ける。
  - **VIVE Hub が止まっている・起動の途中（5.6）に再構築したとき**（PR #37 のレビュー〔3 回目〕の指摘）: 出口の見張りは、開始待ちと止まった印を保つ（5.6 の「再構築」）。再構築の追いつきで読み直す記録のうち、止まった印までのものは、止まる前のセッションのもの。その値は、追いつきのあいだも、終わってからも画面に出さず（7.4 の全体の 5）、追いつきの終わりにも push しない（7.1 の「追いつきのあいだ」）。新しいセッションの開始の行（止まった印より後の時刻）を渡したら、追いつきの中でも外でも、開始待ちを下ろす。ドライバーのファイルだけが作り直されて、両方を読み直すときも同じ。
  - 同じ日に VIVE Hub が起動し直すと、ファイルの途中に `# Log begin at ...` から始まる見出しが足される（メモの 2.3。11/23 は 5 回）。これは追記なので、再構築しない。状態は、そのあとの `ServerStarted`（4.4）で捨てる。
- 判断（どのファイルを、どこから読むか、作り直しか）は `openut_hublog::tail` に入出力なしで書き、ファイルの一覧と読み取りは、アプリが渡すトレイト（例: `LogSource { list(), read_at(name, offset, max) }`）を通す。`tail` は、読んだバイト列のほかに、「再構築する（理由）」「読み終えた」（4.5。追いつきの終わりは、そこまでに読んだサーバーの記録をエンジンに渡し終えたときで、アプリが決める）「次のファイルに移った（日が飛んだか）」「切り替わったあとの書き足し」「ログが見つからない」を順に返し、アプリはそれをエンジン（`rebuild`、`set_catching_up`）に渡す。テストでは、メモリ上の偽物を渡す。

### 4.2 追記分だけ読む（UTF-16）

- ファイルは読み取り専用で開く（`std::fs::OpenOptions` の `read(true)` だけ）。読むたびに開いて、前回の位置から最後まで読み、閉じる。開いたままにしないので、VIVE Hub がファイルを消したり名前を変えたりするのを妨げない。Windows では、Rust の標準ライブラリの既定の共有モードが `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`（Rust 1.100.0 の `library/std/src/os/windows/fs.rs` の `share_mode` の説明で確認）なので、書いている VIVE Hub を妨げない。VIVE Hub 側が読み取りの共有を許しているかは未確認（U-13）。
- 1 回に読む量は 4 MiB まで。それより多く残っていれば、待たずに続けて読む（起動時の追いつき）。
- **解読**（`openut_hublog::decode`。入出力なし）は、バイト列を少しずつ受け取って**記録**（ログの 1 件）を返す。
  - 文字コードは、ファイルの先頭で決める: `FF FE` なら UTF-16LE（BOM は捨てる）、`FE FF` なら UTF-16BE、`EF BB BF` なら UTF-8、BOM がなければ、先頭の 2 バイト目と 4 バイト目が 0 なら UTF-16LE、そうでなければ UTF-8。**実物は、3 種類の 18 ファイルとも UTF-16LE で BOM つき**（メモの 2.1）。ほかの形も読めるのは、再生テストの合成ログを読みやすく書くため（10.1）と、ほかの版で変わっても止まらないため。
  - UTF-16 で、読んだバイト数が奇数なら、最後の 1 バイトを次回に持ち越す。サロゲートの片方で終わったら、それも持ち越す。対にならないサロゲートと、UTF-8 の壊れた並びは、置き換え文字（U+FFFD）にする（止まらない）。解読は標準ライブラリ（`char::decode_utf16`、`String::from_utf8_lossy` の考え方）で書き、クレートを足さない。
  - **記録の終わり**（メモの 2.1）: 実物では、記録の終わりは CRLF で、**本文の中に LF だけの改行が入ることがある**（サーバーの `Save map creation time: ... 2026\n.`、ドライバーの `Wait for HID data got timeout.\n` など）。LF で切ると 1 件が 2 つに割れ、2 つ目が先頭のない行になる。そこで、ファイルごとに**改行の型**を、最初の `\n` で決める。
    - 最初の `\n` の直前が `\r` なら **CRLF の型**: `\r\n` で記録を切る。`\r` の付かない `\n` と、`\n` の続かない `\r` は、本文の文字として記録に残す（捨てない。匿名化の道具〔L-1〕が、行の形を変えずに書き戻せるように）。実物はこの型（どのファイルも、先頭が `#` の行で、CRLF で終わる。メモの 2.3）。
    - 最初の `\n` の直前が `\r` でなければ **LF の型**: どの `\n` でも切り、直前の `\r` は捨てる。合成のログを LF で書いたとき（10.1）のため。
    - 型は、そのファイルの世代のあいだ変えない。最初の `\n` までは記録を出さない（最初の記録が終わっていないため）。`Decoder` は、決めた型を返せるようにする（匿名化の道具が書き戻しに使う）。
    - 最後の記録の終わりがなければ、次回まで持ち越す（CRLF の型で、`\r` で読み終えたときも、次の `\n` を待つ）。
  - 1 件が 64 KiB を超えたら、そこで切って 1 件として流し、「長すぎる行」として数える（最も長い記録は、状態の行や姿勢の行で 1 KiB に届かない。測っていないが、見本の長さから）。

### 4.3 行の先頭

実物の形（メモの 2.2。3 種類のファイルで同じ）:

```
[2026-11-27 22:53:33.658 +9 0xF88 WARN OETServer::ProcessDeviceMsg] [3976] Wifi info is not yet available.
```

- 左から: 日付、時刻（ミリ秒 3 桁）、ずれ、スレッド番号（16 進）、レベル、関数名、`]`、`[スレッド番号（10 進）]`、本文。
- 時刻は PC の現地の時刻。`0x....` はスレッド番号の 16 進で、後ろの `[....]` の 10 進と全件で同じ値（保存しない）。レベルは `INFO`、`WARN`、`ERROR` が出ていた。関数名に空白は入らず、**空のことがある**（`INFO ]`）。**本文が空のことがある**。

解析（`openut_hublog::header`）は寛容にする（ほかの版で少し変わっても読めるように）:

1. `#` で始まる行は `Meta`（4.3 の終わり）。
2. `[` で始まり、日付 `YYYY-MM-DD`、空白、時刻 `hh:mm:ss.mmm`（ミリ秒は 1〜6 桁を受け付け、ミリ秒に丸める）が読めれば `Entry`。読めなければ `NoHeader`。
3. 時刻のあとの語が `+` か `-` で始まれば、ずれ（時の 1〜2 桁、任意で `:mm`）として読む。読めない形（例: `+5.5`）なら、ずれを 0 として `offset_known = false` にする。`+` でも `-` でも始まらなければ、ずれのない行として、ずれを 0、`offset_known = false` にし、その語から 4 に進む。`offset_known = false` の行は、このあとの「ずれの補い」で時刻を直し、`bad_offset` に数える（4.7）。
4. そのあと、最初の `]` までを空白で区切り、1 つ目を識別子（保存しない）、2 つ目を `level`、残り（空白を含んでもよい）を `func` とする。足りなければ空の文字列。
5. `]` のあとに空白と `[...]` があれば、その中を `thread`。
6. 残りの先頭の空白を除いたものを `body`（空でもよい）。末尾の空白と、本文の中の LF（4.2）は残す。

**ずれの補い**（`openut_hublog::header::OffsetFill`。入出力なし）: 3 の「ずれを 0」は、行の先頭の解析の結果で、そのまま時刻にはしない。ずれのある行とない行が混ざっても、時刻が飛ばないようにする。

- **サーバーとドライバーで 1 つ**持つ（2 つのファイルは同じ PC の同じプロセスが書くので、時間の基準は 1 つのはずのため。4.6）。`Entry` を、行の種類の判定（4.4）と 2 つの列の合わせ（4.6）より前に、どちらの列の記録かを付けて通す（合わせるときに時刻を比べるため）。
- 持つもの: **基準のずれ** B（はじめは「未定」。決まると「値 o」か「ずれなし」）と、B が値に決まったときの値 B₀、列（サーバーの記録の並び、ドライバーの記録の並び）ごとの「前の読めたずれ」P_srv、P_drv（はじめはなし）。
- **B を決めるのは、サーバーの列の最初の記録だけ**（読み直しのあとの最初の `Entry`）。ずれが読めれば B を値（B₀ = そのずれ）に、読めなければ「ずれなし」にする。ドライバーの記録では決めない（エンジンの時刻を進めるのはサーバーの記録で〔3.7〕、サーバーの並びは見張りでも再生でも同じため。ドライバーの記録が先に読めても、決め方が変わらない）。
- **B が「ずれなし」**: 両方の列の全部の行を 0 で直す（ずれの読めた行も 0 にする。下の理由）。
- **B が値**: ずれの読めた行（`offset_known = true`）は、その行のずれで直し、その列の P をそのずれにする（夏時間などで変わっても、行ごとに書かれたずれで直る）。ずれの読めない行は、**その列の P を引き継いで**、地方時から `at` を直し、`offset` をその値にする。その列にまだ P がなければ（ドライバーの列の最初のほうの行）、B₀ を使う。
- **B が未定のあいだのドライバーの記録**（サーバーの列にまだ記録がない）は、直さずに持っておき、B が決まったら、順に直して渡す。見張りは、サーバー、ドライバーの順に読む（4.6）ので、持つのは、サーバーの最初の読み取りに記録がなかったとき（ファイルが `#` の行だけのとき）だけ。VIVE Hub は `#` の行のすぐあとに `Log Start` の記録を書く（見本の 1 章）ので、ふつうは起きない。持つのは 2 万件（ドライバーの 1 日の最大 18,775 行より多い）までにし、超えたら古いものから捨てて `driver.bad_offset` に数える（止まらないため）。再生で、最後までサーバーの記録がなければ、B を「ずれなし」にして、持っていた記録を渡す。
- **例 1**（レビュー〔1 回目〕の指摘の 3 行。サーバー）: 地方時 14:00:00 `+9`、14:00:01（ずれの語なし）、14:00:02 `+9` は、UTC の 05:00:00、05:00:01、05:00:02 になる（時刻が戻らず、期限も日付も飛ばない）。
- **例 2**（レビュー〔2 回目〕の指摘。片方の先頭だけ欠ける）: サーバーの最初の記録が `+9`、ドライバーの最初の記録にずれの語がなく、そのあとはどちらも `+9`。B は `+9` に決まり、ドライバーの最初の記録は B₀（`+9`）で直る。同じ地方時 14:00:00 のサーバーの切断とドライバーの予告は、どちらも UTC の 05:00:00 になり、R4 の条件 5（6.2）で予告が見つかる。逆に、サーバーの最初の記録にずれがなければ、B は「ずれなし」で、両方の列が 0 で直るので、やはり同じ時刻どうしで比べられる。
- 「ずれなし」に決めたあとに読めたずれを使わないのは、時刻が途中で 9 時間の段にならないようにするため（戻る段は、4.6 の「時刻が戻る行」の扱いで、そのあとの何時間分もの行が 1 つの時刻にまとまってしまう）。ずれを書かない版のログ（全部の行が「ずれなし」）は、時刻が地方時のまま一定にずれるだけなので、見張りの時刻の進め方（4.6）の d で補われる。B が値でも「ずれなし」でも、行の地方の日付（8.1）は、書かれた日付に戻る（`at` と `offset` を同じ値で直すため）。
- 初期化するのは、読み直しのとき（起動と再構築。4.1、4.5）だけ。0 時の切り替わり、日が飛んだとき、VIVE Hub の起動し直し（`# Log begin at`）では持ち越す（同じ PC のため）。再生（10.2）は、1 つを両方の列の最初のファイルから通す（サーバーの列の最初の記録で B を決めてから、2 つの列を直す）。**どの行の直し方も、サーバーの列の最初の記録と、その行の列の中の並びだけで決まり**、2 つの列が読まれる順（読み取りの区切り）に左右されないので、見張りと再生で同じになる。
- 数え方: `offset_known = false` の行（補ったものも含む）と、「ずれなし」に決めたあとに読めたずれを捨てた行を、その列の `bad_offset`（ドライバーは `driver.bad_offset`）に数える（4.7）。
- 確かめていないこと: 夏時間のある時間帯で、ずれの読めない行が、ずれの変わり目の近くにあった場合（引き継ぐ P が古い値で、1 時間ずれうる）。6 日分では、ずれの読めない行は 0 件（11 万件すべてが `+9`）で、ふつうは起きない。`bad_offset` が 0 でない日は、集計の `source` で分かる。

- `+9` を UTC からのずれ（時）と読むのは推定のまま（メモの 2.2。11 万件すべてが `+9`）。読み違えていても、列の中で一定なら、見張りの時刻の進め方（4.6）は壁時計とのずれ d を測って補うので、検出の時刻は狂わない。集計の日付は、行の地方の日付で決まる（8.1。補ったずれで、行に書かれた地方の日付に戻る）。30 分単位のずれがどう書かれるかは分からないので、読めないときに止まらず、上の補いで続ける（W-9）。
- `NoHeader` の行は、規則には渡さず数えるだけにする。CRLF で記録を切る（4.2）ので、実物では出ない見込み（6 日分で、`#` の行のほかに、記録の形をしていない行はなかった。メモの 2.3）。

**`#` で始まる行**（`Meta`。メモの 2.3）: プロセスが起動するたびに `# Log begin at <時刻>` と設定の 13 行（`# WriteTimeInterval=5000` など）が書かれ、日付が変わって作られたファイルは `# Log rotate by date at <時刻>` で始まる。どちらも出来事にはせず、`meta` に数えるだけ（起動は、そのあとの `Tracker Server Starts.` の記録で知る。4.4）。`#` の行の時刻は、ミリ秒の桁をそろえない（`.40` は 40 ミリ秒）ので、読むことがあっても、記録の時刻と同じ読み方をしない。

### 4.4 行の種類と出来事

本文の判定（`openut_hublog::record`）は、下の表の**本文の先頭の形**で行う（綴りは、メモの 3.1〜3.3 と見本のとおり。6 日分で、大文字と小文字の揺れはなかった）。`N`・`V`・`K`・`S` は整数（負の値もありうる）。番号 N は、どの行でも同じドングルのスロットの番号（3.2）。表にない本文は `Other`（数えるだけ）。

**サーバーのファイル**（`ViveTrackerServer_*.txt`）

| 種類 | 関数名（参考） | 本文の先頭の形 | 取り出すもの | 出来事 |
|---|---|---|---|---|
| ServerStart | `main` | `Tracker Server Starts.` | — | `ServerStarted` |
| ServerRelease | `OETServer::Release` | `Release begin` | — | `ServerStopped` |
| DongleAdded | `OETServer::OnDongleAddedCB` | `Dongle is added.`（あとに ` version: (...)` が続く） | — | `DongleAdded` |
| DongleRemoved | `OETServer::OnDongleRemovedCB` | `Dongle is removed.` | — | `DongleRemoved` |
| TrackerAdded | `OETServer::AddTrackerCB` | `Add new tracker: (N), connection type: V` | N、V | `TrackerAdded`（`usb` は V が 2 のとき） |
| TrackerRemoved | `OETServer::RemoveTrackerCB` | `Remove tracker (N), connection type: V, power state:V `（`power state:` のあとに空白がない） | N、V（connection type） | `TrackerRemoved`（`usb` は V が 2 のとき） |
| DeviceInfo | `OETServer::GetDeviceInfoAndResponse` | `Response - cmd type: ` で始まり、`, sn: (` を含む（含まなければ `Other`）。`, device index: N,` と `, sn: (S)`、任意で `, model name: (VIVE Ultimate Tracker V)` と `, fw version: (X)` | N、S（括弧の中。空なら出来事にしない）、V、X（下の注） | `SerialKnown`（V が `hub_number`、X が `firmware`） |
| HostMode | `Tracker::UpdateSwitchingMode` | `Tracker (N) - switch to HOST mode.` | N | `HostAssigned` |
| HostWifiQuery | `OETServer::TrackerSwitchAndResponse` | `Query wifi from tracker N`（括弧なし） | N | `HostWifiQueried` |
| HostWifiGot | `OETServer::ProcessDeviceMsg` | `Got host wifi info.`（**番号なし**） | — | `HostWifiObtained { host: None }` |
| WifiAssign | 同上 | `Assign wifi info to client tracker (index: N).` | N | `WifiAssigned` |
| WifiNotYet | 同上（`WARN`） | `Wifi info is not yet available.`（**番号なし**） | — | `WifiRequestUnanswered { tracker: None }` |
| ClientLink | 同上 | `Client tracker (index: N) is connected to host: V` | N、V（0 か 1。ほかは読めなかった行） | `ClientLink`（V が 1 なら `connected`） |
| DeviceStatus | `OETServer::DeviceStatusToVHServer` | `display index: ` で始まり、`key: value` を `, ` で並べたもの（下の注） | `device index`、`connected`、`map status`、`map state`、`is host`、`bIsConnectedOnUSB`（任意）、`percentage`（任意）、`charging`（任意） | `Status` |
| ServiceStatus | `OETServer::ServiceStatusToVHServer` | `[ServiceStatusToVHServer] ` で始まり、`key: value` を `, ` で並べたもの | `dongle connected`（0 か 1。ほか・ないなら読めなかった行） | `DongleReport`（1 なら `connected`） |
| Pose | `Tracker::CopyFromDriverThreadProc` | `(SteamVR: S)(N)[K]Pose status is changed: Pose data - status: V,` | N、V | `Pose`（2 → `Ok`、3 → `Lost`、4 → `RecentlyLost`、ほか → `Other(V)`） |
| OetLost | `OETServer::OnTrackerErrorNotification` | `OET LOST TRACKING! index: N` | N | `Oet(LostTracking)` |
| OetReloc | 同上 | `OET reloc back. index: N` | N | `Oet(RelocBack)` |
| OetOverspeed | `OETServer::OnTrackerStatusIndicatorUpdate` | `[N] OET OVERSPEED!` | N | `Oet(Overspeed)` |
| Command | （下の表） | （下の表） | N（あれば） | `PcCommand` |

**PC からトラッカー・ドングルへの命令にあたる行**（メモの 3.2。種類 Command。`PcCommand` の `kind` は左の列の名前）

| `kind` | 本文の先頭の形 | 番号 |
|---|---|---|
| `client_mode` | `Tracker (N) - switch to CLIENT mode.` | N |
| `start` | `Start tracker - power state :`、`Start tracker. state:`、`Start host tracker. ret:` | なし（全台） |
| `stop` | `Stop tracker.`、`Fail to stop tracker, err:` | なし（全台） |
| `pair` | `Pair tracker, isPair: V, device index:N`（N が負なら番号なし） | N か、なし |
| `unpair` | `Unpair All`、`Cancel pairing on dongle.` | なし（全台） |
| `wake_dongle` | `Wake up dongle: (N).` | N |
| `wake_dongle` | `Wake up dongle.` | なし（全台） |
| `standby_dongle` | `Set dongle into standby.` | なし（全台） |
| `power_off` | `PowerOff tracker (N) failed, err: V`、`FAILED TO POWER OFF. index: N` | N |
| `camera_rate` | `Camera frame rate: V`。**関数名が `OETServer::SetCameraFrameRateToAllTrackers` のときだけ**（起動のときの設定の読み込み〔`OETServer::ParseConfig`〕にも同じ本文が出るため） | なし（全台） |

- `HostAssigned`（`switch to HOST mode.`）、`HostWifiQueried`、`WifiAssigned` も命令にあたる（3.4）。命令の失敗の行（`power_off`、`stop` の一部）も、PC がそうしようとした印として、命令に数える。
- **状態の行**（DeviceStatus）: トラッカー 1 台について 1 行（メモの 3.1）。`, ` で区切って `key: value` の組にし、`device index`、`connected`、`map status`、`map state`、`is host` の 5 つのキーの値を整数で読む（どれかがない・読めなければ、読めなかった行）。任意のキー（`bIsConnectedOnUSB`、`percentage`、`charging`）は、なくても読めなかった行にしない。ほかのキー（`tracking status`、`has new fw` など）は使わない。出来事の値:
  - `connected`: `connected` が 1。`host`: `is host` が 1。
  - `usb`: `bIsConnectedOnUSB` が 1 なら `Some(true)`、0 なら `Some(false)`、キーがない・読めなければ `None`（このキーだけは、なくても読めなかった行にしない。接続の経路を補うだけのため。5.1）。見本の状態の行は、どれも 0。
  - `map_ready`: `map state` が 2、または `map status` が 6。値の意味は、初版の調べ（解析ライブラリ pyvut の定数の名前 `MAP_REUSE_OK` などと合う、という記述）による。pyvut（<https://github.com/rocketmark/pyvut>）にはライセンスの表示がないので、定数の表は写さず、意味だけを使う。メモでは、意味を確かめ直していない。
  - `tracking status` は使わない。姿勢の行の値（2・3・4）にない 5・6・7 が出るので、同じ番号づけか分からないため（メモの 3.1）。
  - `battery`（W-11。#27 で足した）: `connected` が 1 で、`percentage` が 1〜100 の整数なら `Some(Battery { percent, charging })`。`charging` は、キーの値が 0 なら `Some(false)`、1 なら `Some(true)`、ほか・ないなら `None`。`connected` が 0 の行（起動の直後の、登録済みの台の行。見本の 1 章では `percentage: 0`）、`percentage` がない・0・範囲の外・数でない行は `None`（分からない）。どちらのキーも、なくても読めなかった行にしない（電池の表示だけに使うため）。
  - **`percentage: 0` を「分からない」とするのは推定**: つながっている台が 0% のまま動き続けることはないと見て、まだ値が来ていない印と読む（0% と見せて、利用者に電池切れと思わせないため）。U-19。M1-13 で、`connected: 1` の行の `percentage: 0` の数と、出る時刻（接続の直後か）を数える。
  - 値の意味（`percentage` が電池の残りの %、`charging` が充電中）は、キーの名前と、出ていた値（見本で 74、98）からの推定（U-19）。
- **電池の残りが出る別の行**: `Peripheral data changed - is charging: V, battery percent: V, ...`（`OETServer::OnPeripheralDataChanged`。見本の 2 章）にも電池の残りが出るが、どのトラッカーの値かが行にない（番号がない）ので使わない（`Other`）。電池の残りは、状態の行の `percentage` だけから取る。
- **`GetDeviceInfoAndResponse` の行の `model name` と `fw version`**（W-11、W-12。#27 で足した）: どちらも `, ` のあとの `key: (値)` の形（見本の 4 章: `sn: (00XX000000001), model name: (VIVE Ultimate Tracker 1), fw version: (1.0.999.178)`）。括弧の中は、閉じ括弧 `)` までとする。
  - `model name` の括弧の中が `VIVE Ultimate Tracker V`（V は 1〜99 の整数）なら `hub_number = V`。ほかの形（ほかの機種の名前、数がない、0、100 以上、キーがない）は `None`。
  - `fw version` の括弧の中が、1〜32 文字の、数字・英字・`.`・`-`・`_` だけなら `firmware` にする。空、長すぎる、ほかの文字を含む、キーがないなら `None`。
  - どちらが `None` でも `SerialKnown` は出し、読めなかった行にしない（3.4）。
  - シリアル番号 S は、1〜32 文字の、ASCII の英数字と `-` だけのときに受け付ける。ほかの文字（空白、制御文字、ASCII の外）を含むときは、読めなかった行（`unparsed`）にする（画面と集計に、おかしな文字を出さないため。見本の置き換えの形 `00XX000000001` は受け付ける）。空なら、これまでどおり出来事にしない。
- **`[ServiceStatusToVHServer]` の行**（W-11。#27 で足した。メモの 3.1: 状態の行と同じ時刻に続くことが多い）: `dongle connected: 1` ならドングルがつながっている、`0` ならつながっていない、という報告として `DongleReport` にする（5.5）。意味は、キーの名前と、出ていた値（見本の 2 章で 1）からの推定（U-20）。ほかのキー（`tracker connection type`、`device counts`、`camera fps` など）は使わない。
- **姿勢の行**（Pose）: 状態が変わったときだけ出る（`Pose status is changed:`。定期的には出ない）。`V` には、接続の直後に意味のなさそうな値（47、58、71 など）や 0 が出る。どれも `Other(V)` にし、止まらない。位置や回転の `-nan` と桁の多い値は読まない（判定に使わない）。
- 本文の先頭が `(SteamVR: ` でも、`]Pose status is changed: Pose data - status: ` を含まない行（`... includes NAN.`。1 日に最大 8,646 行）は `Other`。
- **数が多くて検出に使わない行**（メモの 3.1 の終わり）: `... includes NAN.`、`OnPipeClientError::OnPipeError ...`、`Tracker is not found`。どれも `Other`。サーバーの行は 1 日に最大 15,828 行（毎秒 1 行に満たない）なので、読み飛ばしの特別な仕組みは作らない。判定は、本文の先頭の 1 文字か 2 文字で候補を絞り、表の形と前から比べるだけにする（正規表現のクレートを足さない）。
- 番号や値を読むのは、表の形の `N`・`V` の位置だけ。形の先頭が合ったのに、決まった位置の数が読めない（桁あふれ、数でない）ときは、読めなかった行（`unparsed`。4.7）にする。番号が 0〜4 の外でも読めた数なら受け付ける（台数を決めない。W-9）。
- **同じ出来事の行が 2 回出る**（メモの 2.2。例: `Remove tracker (2), ...` が 3 ミリ秒おいて 2 回）。本文が同じ行を捨てる処理は**しない**（続けて来る `Wifi info is not yet available.` は、1 つ 1 つが別の問い合わせで、捨てると R1 が数え違えるため）。重なりは、状態の側で、同じ遷移を 2 回しないことで吸収する（5.1）。
- `dongle count: V, tracker count: V`、`Tracker N entered sleeping (lost tracking)`、`SteamVR Running is changed to V`、`error code: V received` などは、M1 の規則と表示に使わないので `Other`（数えるだけ）。初版で `Other` にしていた `Dongle is added.` は、ドングルの表示（5.5）のために種類にした。

**ドライバーのファイル**（`ViveTrackerDriver_*.txt`）

| 種類 | 関数名（参考） | 本文の先頭の形 | 取り出すもの | 出来事 |
|---|---|---|---|---|
| PowerOffNotice | `TrackerDongleHID::ParseIndication` | `(N) Tracker is going to power-off.` | N | `PowerOffNotice` |

- ドライバーのファイルのほかの行（9 割以上が `Wait for HID data got timeout.\n`）は、`driver.lines` に数えるだけ（4.7。`other` には入れない）。
- ドライバーの番号 N は、サーバーの番号と同じスロットの番号（メモの 3.3）。予告の行は、同じ出来事のサーバーの `Remove tracker (N)` と同じミリ秒に、同じスレッドで出ていた（見本の 5 章）。

### 4.5 途中から読み始めたとき（追いつき）

- 起動したら（と、作り直したとき。4.1）、次の順に**先頭から**読む。その日の 0 時の状態を組み立て、その日の集計（8 章）も最初から数えるため。サーバーとドライバーのそれぞれについて同じ順に読み、2 つの記録は時刻の順に並べて流す（4.6。再生と同じ）。
  1. **前の日のファイル**: いまのファイルの日付のちょうど 1 日前の日付のファイルが、フォルダーにあれば。0 時をまたいで使っていたときに、0 時の時点の状態（どのトラッカーが追跡中か、ホスト、PC の Wi-Fi 情報、続いているエピソード）を持ち越すため。
  2. **いまのファイル**。
- 前の日のファイルがなければ、いまのファイルの先頭から、不明な状態で始める。`TrackerAdded` なしで出てきた番号は、行の意味から状態を推し量って足し（`inferred`）、PC の Wi-Fi 情報は `Unknown` から始める（5 章）。2 日以上前のファイルは読まない。
- 1 と 2 の読み取りが、起動した時点のいまのファイルの終わりに（サーバーとドライバーの両方で）着いた時を「**読み終え**」と呼ぶ。読み終えたうえで、それまでに読んだサーバーの記録をすべてエンジンに渡し終えるまでを「**追いつき**」とし、エンジンに `set_catching_up(true)` を渡す。ドライバーを待つ決まり（4.6 の 2・3）でサーバーの記録を持っているあいだは、読み終えても追いつきを続ける（待ちは、読み終えてから有限の時間で終わる。4.6 の 3）。追いつきのあいだも、状態・規則・集計はふだんどおり進めるが、配信（7.1）は通知を出さず、集計は書き出さない。
- 追いつきが終わったら、まず見張りの時刻（4.6。ドライバーを待つ決まりの D を超えない）まで進め、**その時点で続いている**検出（`Raised` のあと、`Resolved` も `Ended` も出ていないもの）だけを、ふつうの配信の決まりで出す。終わってしまった検出は出さない（集計には数える）。判定がまだの R4 の切断（期限 D がまだ来ていないもの）は、追いつきの終わりで判定せず、そのまま持ち越す（ふつうの見張りの中で、4.6 の 2 の待ちのあとに判定する。起動・再構築の境目でも、ドライバーの予告を待つため）。
- 前の日の集計は、追いつきで閉じても、その日の `final: true` の集計のファイルがすでにあれば書き直さない（0 時をまたいで動いていた回の結果を残すため。9.3）。なければ書く。

### 4.6 まとめて書かれることと、時刻の進め方

VIVE Hub は、まとめてファイルに書く。ヘッダーの `WriteTimeInterval=5000` から、**5,000 ミリ秒ごと**と読める（メモの 2.4。推定で、ファイルが伸びる様子は測っていない。U-8）。3 種類のファイルとも同じ設定で、サーバーとドライバーは同じプロセス（`ViveTrackerServer.exe`。見本で、同じスレッド番号が両方のファイルに出る）が書く。検出は最大でその間隔だけ遅れる。

- エンジンの時刻は、サーバーの行の時刻で進む（3.7）。規則の「何秒たっても〜しない」は、エンジンが期限を持ち、`advance_to` で期限を過ぎたものから順に判定する（期限ちょうどの入力は間に合ったものとする。3.7、6.1）。
- ログが静かなあいだも期限を判定できるよう、見張りのループは、行がなくても 500 ms ごとに `advance_to(見張りの時刻)` を呼ぶ。見張りの時刻は、「これより前の時刻の行は、もう読み終えた」と見なせる時刻。

  見張りの時刻 = max(最後に読んだ行の時刻, 今の壁時計 − d − L)

  - d: 壁時計とログの時刻のずれ。読んだまとまりごとに「読んだときの壁時計 − そのまとまりの最後の行の時刻」を測り、直近 5 分の最小値を使う（直近 5 分に測ったものがなければ、最後に測った値）。追いつき（4.5）で読んだ分は古い行なので、d の測定に使わない。ただし、**読み終えた時（4.5）に 1 回だけ**、「読み終えた時の壁時計 − サーバーで最後に読んだ記録の時刻」を d の測定として足す（**d₀**）。最後の記録が書かれてから時間がたっていれば、d₀ は実際のずれより大きく、見張りの時刻を遅らせる側にだけ効く（早すぎる判定にはならない）。あとで測った値が小さければ、最小値でそちらに替わる。d₀ があるので、読み終えたあとにサーバーのファイルが伸びなくても、見張りの時刻は「最後の記録 − L」から壁時計の速さで進み、期限は有限の時間で判定される。d がまだなければ（読んだ記録が 1 つもない）、見張りの時刻は最後に読んだ行の時刻のまま。ずれ（4.3 の `+9`）の読み方が違っても、PC の時計がずれていても、ここで吸収する。
  - L: まとめて書かれる間隔より長い余裕。**仮の値 10 秒のまま**（ヘッダーの 5,000 ミリ秒の 2 倍。読み取りの間隔 500 ms と、書く時刻の揺れを足しても足りる）。短すぎると、まだ書かれていない行が来る前に期限を判定してしまう。長すぎると、期限の検出がその分遅れる。決め方: 見張り（M1-9）が、読んだまとまりごとの「読んだときの壁時計 − そのまとまりの最後の行の時刻」をアプリのログに残す。オーナーの試し（14 章の 1）と、使った日のアプリのログから、その最大値に数秒を足した値にする。
  - d と見張りの時刻は、**サーバーのファイルだけ**で測る（ドライバーのファイルは、トラッカーがつながっていないと何時間も伸びないことがあり、遅れて届いた行で d を測り違えないため）。
  - 読んだ行がまだ 1 行もなければ、時刻を進めない。
- **2 つのファイルの並べ方**: 見張りのループは、500 ms ごとに、サーバー、ドライバーの順に伸びた分を読み、記録の先頭を解析し、2 つの列で 1 つの時間の基準でずれを補ってから（4.3。基準が決まる前のドライバーの記録は、そこで持たれる）、2 つの列を合わせて出来事にする。並べ方の決まりは次の 3 つ。再生（10.2）は、2 種類のファイルの全部を、時刻の順に合わせて流す（先頭どうしを比べて早いほうを出し、同じ時刻ならサーバーを先にする）。
  1. **エンジンの時刻を進めるのは、サーバーの記録だけ**（3.7）。ドライバーの記録は、時刻を進めず、状態も変えない（`PowerOffNotice` は元の時刻で覚え、ほかの行はその行の日に数える）。そのため、ドライバーの記録は、読んだらすぐ渡してよい（サーバーより遅れていても、進んでいても、結果は変わらない）。サーバーの記録は、ファイルの順に渡す。先頭の読めない行と `#` の行は、同じ列の前の記録に続けて渡す（時刻を持たないため）。
  2. **ドライバーを待つ**: エンジンの `driver_needed_through()` が D を返し、ドライバーの見張りの時刻 W_drv が D 以下のあいだは、サーバーの記録のうち時刻が D より後のものを渡さずに持ち、`advance_to` も D までにする（`advance_to(min(見張りの時刻, D))`。D ちょうどの期限は残る。3.7）。W_drv が D を超えたら、持っていた記録を順に渡す（渡した記録で新しい D ができれば、また同じに待つ）。こうすると、R4 の判定（期限 D）は、D 以下の時刻のドライバーの記録がすべて届き、D より後のサーバーの記録が 1 つも入っていない時点で行われ、再生と同じになる。D が、いま数えている日の最後の時刻のときは、日を閉じる前に、その日のドライバーの行がそろう（8.1）。
     - W_drv = max(ドライバーの最後に読んだ記録の時刻, 今の壁時計 − d − L)。d と L は、サーバーの見張りの時刻と同じ値（d はサーバーで測る。上）。ドライバーのファイルが静かでも、壁時計で進むので、待ちは D から L ほどで終わる（有限）。d がまだなければ、W_drv はドライバーの最後に読んだ記録の時刻（サーバーの見張りの時刻も同じく進まないので、待ちが増えるわけではない）。
     - ドライバーのファイルがないとき（4.1）は待たない（W_drv を限りなく先とする）。R4 の条件 5 は確かめられない（6.2）。
     - 待つのは、切断のあと R4 の判定までと、0 時の前後だけなので、ほかの案内（R1 など）が遅れるのは、その間に重なったときだけ（多くて L）。
  3. **追いつき**（4.5。起動と再構築）: 読み終える（4.5）までは、W_drv を「ドライバーで最後に読んだ記録の時刻」とする（壁時計は使わない。d₀ がまだないため）。読み終えたら、2 の W_drv（d₀ を含む d）に戻す。
     - 2 の待ちでサーバーの記録を持っているあいだは、ドライバーのファイルを先に読み進める（読み切れない分〔4 MiB を超える分〕があれば続けて読む）。ドライバーが**いまのファイル**（それより新しい日付のドライバーのファイルがないもの。追記が続きうる）の終わりに着いていれば、サーバーのファイルを読み進める（持つ記録は増えるが、読み終えるため。多くて追いつきで読む 2 日分）。前の日のドライバーのファイルの終わりでは、次のファイルに移るだけで、待ちは解かない。
     - **追記中のファイルの終わりでは、待ちを解かない**。初版の「追いつきでドライバーのファイルの終わりに着いたら待たない」はやめた。起動の直前の切断の予告が、まだ書かれていないことがあるため（レビュー〔2 回目〕の例: 切断 t、起動 t + 5.3 秒。サーバーは t + 5.1 秒の行まで読め、ドライバーの t + 0.9 秒の予告は t + 5.9 秒に追記される。待たないと、期限 t + 5 秒で `Raised` を出してしまい、再生では除かれる）。
     - 待ちの終わり: 読み終えた時の壁時計を w₀、サーバーで最後に読んだ記録の時刻を T_srv とすると、d₀ = w₀ − T_srv なので、W_drv は壁時計 w で少なくとも T_srv + (w − w₀) − L になる。そのため、期限 D の待ちは、遅くとも w = w₀ + max(0, D − T_srv) + L で終わる（D が T_srv − L より前なら、読み終えた時に終わる。その時刻までのドライバーの記録は、サーバーの最後の記録より L 以上前のもので、書き終わっていると見なす）。R4 の期限は切断の 5 秒あとで、切断は T_srv 以前なので、多くて読み終えてから 5 秒 + L。日の終わりの D は、それより後のサーバーの記録を持っているとき（D < T_srv）だけ待つので、多くて L。読み終えたあとにサーバーが伸びなくても有限。上の例では、w₀ ≈ t + 5.3 秒、T_srv = t + 5.1 秒なので、待ちは t + 15.2 秒ごろに終わり、t + 5.9 秒に追記された予告がその前に入って、R4 は除かれる（再生と同じ）。
     - 再生と違いうるのは、ドライバーの記録が、サーバーの最後の記録より L を超えて遅れて書かれていたときだけ（下の `driver.late`）。
- L より遅れて届いたドライバーの記録（読んだ時点で、時刻がその前の W_drv より前だったもの。W_drv の見込みが外れたもの）は、`driver.late` に数える（4.7）。再生と違いうるのは、それが判定を済ませた R4 の範囲に入る予告だったときと、閉じた日の行だったときだけ（閉じた日の行は、その日には足さず、`driver.late` だけに数える）。
- サーバーの記録で時刻が戻るもの（L より遅れて届いた行、PC の時計の変更）は、いまの時刻に起きたものとして扱い、「時刻が戻った行」（`time_regressions`）として数える。6 日分のサーバーの 6 ファイルでは、ファイルの中で時刻が戻ることはなかった（同じミリ秒が続くことは多い。メモの 2.2）。ドライバーの記録は時刻を進めないので、時刻が戻る行にはならない（遅れは、上の `driver.late`）。
- 再生テストでは壁時計を使わない（10.2）。

### 4.7 想定と違う行

止まらずに読み続ける（受け入れの基準）。どの段でも `panic` しない。

| 起きること | 扱い | 数える名前 |
|---|---|---|
| 行の先頭が読めない | 規則に渡さない | `no_header` |
| `#` で始まる行（4.3） | 規則に渡さない | `meta` |
| ずれが読めない（4.3） | 同じ列の前の読めたずれを引き継ぐ（その列にまだなければ、サーバーの最初の記録で決めた基準のずれ。サーバーの最初の記録のずれが読めなければ、両方の列を「ずれなし」にする。4.3 の「ずれの補い」） | `bad_offset`（ドライバーは `driver.bad_offset`） |
| 種類は分かったが、番号や値が読めない | 規則に渡さない。最初の 20 件まで、アプリのログに本文を残す | `unparsed` |
| 表にない本文 | 規則に渡さない | `other` |
| ドライバーのファイルの記録（予告の行を含む全部） | 予告の行だけを出来事にする | `driver.lines`（ほかの数も `driver.` の下に、同じ名前で） |
| 対にならないサロゲート、壊れた UTF-8 | 置き換え文字にして続ける | `replaced_chars` |
| 64 KiB を超える行 | 切って続ける | `long_lines` |
| サーバーの時刻が戻る行 | いまの時刻として扱う | `time_regressions` |
| ドライバーの記録が L より遅れて届いた（4.6） | ふつうに渡す（判定を済ませた R4 と、閉じた日には効かない） | `driver.late` |
| 切り替わったあとの古いファイルの書き足し | 読まない（起動し直すと、前の日のファイルとして読まれるので、この数が 0 でない日だけは、起動し直したときと結果が違いうる） | `late_writes` |
| いまのファイルが作り直された（縮んだ、置き換えられた、消えた） | 再構築する（4.1） | `rebuilds` |

- 数は、日ごとの集計（8 章）の `source` に書く。`unparsed` が増えたら、ログの書式が変わった印なので、見張り役の具合を「形が違う」にする（案内ではない。利用者への伝え方は 7.4 の「仕組みの側の不具合」。行の数と例は、アプリのログにだけ書く）。
- **綴りが変わった版**では、表の行がすべて `other` になり、`unparsed` は増えない。そのため、その日のサーバーの記録が 2,000 件（仮）を超えたのに、表の種類（`Other` でないもの）が 1 件もなければ、同じく「形が違う」にする（7.4）。6 日分では、1 日の記録は 2,002〜15,828 件で、状態の行（`DeviceStatusToVHServer`）がトラッカーをつないでいない日にも出る見込み（起動の直後に、登録済みのトラッカーの行が出る。メモの 3.5）。
- アプリのログに残す本文には、シリアル番号などが入りうる。アプリのログは利用者の PC にだけ置き、送る機能（L-4）では匿名化を通す。

### 4.8 ログの書式で確かめていない点

初版の U-1〜U-16 のうち、メモで分かったもの（U-1 文字コード、U-2 改行、U-4 複数行の本文、U-6 番号、U-7 行の出方、U-10 開始と終了、U-14 フォルダー、U-15 行数と大きさ、U-16 命令の行）は、この版の本文（4.1〜4.4、5 章、9.2、12 章）に入れたので、表から消した。対応はメモの 5 章の表のとおり。**残っているもの**:

| 番号 | 確かめていないこと | 影響する場所 | 確かめ方 |
|---|---|---|---|
| U-3 | `+9` が UTC からのずれ（時）か。30 分単位のずれの書き方 | 4.3（読めなければ 0 で続ける）。検出の時刻は 4.6 の d で補う | ほかの時間帯の PC のログ（ログの送信〔L-4〕が入ってから） |
| U-5 | 状態の行の値の意味（`map state: 2`、`map status: 6` が地図の準備ができた印か。`connected: 0` の状態の行が、つながっている台に出るか。`bIsConnectedOnUSB` が USB の接続を表すか〔見本ではどれも 0〕）。姿勢の行の値 0 の意味 | 4.4、5.1 | 匿名化した実物のログ（L-2）の再生で、`map_ready` から追跡中までの経過と、5.1 の「順序の合わない出来事」の数を見る（M1-13） |
| U-8 | まとめて書かれる実際の間隔（ヘッダーの 5,000 ミリ秒は推定） | 4.6 の L | 14 章の 1（オーナーの試し）。M1-9 のアプリのログ |
| U-9 | 大きさが 15 MB を超えたときの動き | 4.1 | 起きたときのアプリのログ（4.1。`_N` のファイル） |
| U-11 | 電源オフの予告（ドライバーの行）のあった切断が、利用者の操作（ボタンで電源を切った）によるものか、トラッカーが自分で切ったもの（電池切れ、スリープ）を含むか | 6.2 の R4 の条件 5 | 14 章の 2（オーナーの試し） |
| U-12 | ドングルが抜けたときに、トラッカーの `Remove tracker` が出ないか（1 回の観察だけ） | 5.1（`DongleRemoved`）、R4 の条件 4 | 匿名化した実物のログの再生。次にドングルを抜いたときのログ |
| U-13 | VIVE Hub が書いている最中のファイルを、ほかのプログラムが読めるか（共有モード。`OpenType=1`、`WriteThrough=1` の意味が分からない） | 4.2。読めなければ設計の作り直しが要る | 14 章の 1（オーナーの試し） |
| U-17 | ほかの版（VIVE Hub、ランタイム、ファームウェア）と、ほかの PC で、綴りと形が同じか。メモは 1 台・6 日・1 つの版 | 4.4。変わったら 4.7 の「形が違う」で気づく | ログの送信（L-4）が入ってから |
| U-18 | `model name: (VIVE Ultimate Tracker N)` の N が、VIVE Hub の画面に出る番号と同じか。状態の行の `display index`（0〜2）に 1 を足した数と同じか（見本では、`device index: 2` の台が `display index: 0` と `VIVE Ultimate Tracker 1`。メモの 3.5）。1 つの接続のあいだに変わるか。VIVE Hub の画面が、台をどの順に並べているか。**分かったこと**（PM が 2026-11-23〜28 のサーバーのログ〔匿名化の道具を通したもの〕で数えた。PR #37 の PM のコメント 2026-11-28 18:36）: **台ごとに固定ではない**。3 台とも 6 日のうちに 1・2・3 のすべてになり（続く 2 行で番号が変わった回数は、台ごとに 13〜15 回）、同じ晩のうちにも変わる。つなぎ直すたびに振り直されているように見える（何で決まるかは調べていない）。`model name` が読めない行と、`VIVE Ultimate Tracker N` 以外の形の行は、310 行のうち 0 行 | 3.2（番号はつながっている台だけ）、5.1、5.4、7.4（呼び名と並べ方） | オーナーの確認（M1-11 のあと、VIVE Hub の画面と OpenUT の画面の番号と並びを見比べる。14 章）。匿名化した実物のログ（M1-13。番号が 1 つの接続のあいだに変わった数と、`display index` + 1 と同じだった割合） |
| U-19 | 状態の行の `percentage` が電池の残り（%）か、`charging` が充電中か。つながっている台の `percentage: 0` が、いつ出るか（接続の直後だけか） | 4.4（0 を分からないとする）、7.4 | 匿名化した実物のログ（M1-13。`connected: 1` の行の `percentage: 0` の数と、接続からの時間）。オーナーの確認（VIVE Hub の電池の表示と見比べる） |
| U-20 | `[ServiceStatusToVHServer]` の行の `dongle connected` が、ドングルの接続を表すか。ドングルを抜いたあとも、この行が出るか | 4.4、5.5 | 匿名化した実物のログ（M1-13。`Dongle is removed.` の前後の値）。オーナーの確認（ドングルを抜いて 1 分待つ。14 章） |

## 5. 状態の遷移（W-2）

状態（`openut_core::state`）は、出来事を受けて表を更新するだけで、時刻を自分で進めない。

### 5.1 トラッカー

行は `TrackerKey` ごと。最初に出てきた出来事で行を足す（台数を決めない）。

| 出来事 | 前の状態 | 次の状態 | ほかに変えること |
|---|---|---|---|
| `TrackerAdded(N)` | 未接続 | 接続 | 新しい接続の番号 `conn` を付ける（下の「接続」）。役割を `Unknown`、`tracked_since_connect = false`、`inferred = false`、シリアル番号をなしに。接続の経路 `link` を、`usb` なら `Usb`、でなければ `Dongle` |
| `TrackerAdded(N)` | 未接続以外 | 下の注の「重なり」 | |
| `TrackerRemoved(N)` | 未接続以外 | 未接続 | 役割を `Unknown`、シリアル番号をなしに（スロットは別のトラッカーに使われるため）。`link` を `Unknown` |
| `TrackerRemoved(N)` | 未接続 | 変えない | 数えない（同じ出来事の 2 つ目の行。メモの 2.2） |
| `SerialKnown(N, S, hub, …)` | 未接続以外 | 変えない | N のシリアル番号を S にする。ほかの番号に S が付いていれば、そちらから外す（その番号の `hub_number` もなしに）。N の `hub_number` を、hub が `Some` ならその値にする（`None` なら、N のシリアル番号が前から S だったときだけ前の値を残し、ほかはなしに）。台帳（5.4）を更新する |
| `HostAssigned(N)` | 接続、Wi-Fi 待ち | 地図同期中 | ホスト（5.3）を N にする（役割の変え方も 5.3）。ほかの「接続」のトラッカーを Wi-Fi 待ちにする（役割は変えない） |
| `HostAssigned(N)` | 地図同期中、位置取り直し中、追跡中 | 変えない | ホストの変え方は上の行と同じ。すでに動いているトラッカーへの 2 度目の命令（不具合の始まり）で、状態を巻き戻さないため |
| `WifiAssigned(N)` | 接続、Wi-Fi 待ち | Wi-Fi 待ち | 役割を `Client` |
| `WifiRequestUnanswered` | — | 下の注の「番号のない出来事」 | |
| `ClientLink(N, true)` | 接続、Wi-Fi 待ち | 地図同期中 | 役割を `Client` |
| `ClientLink(N, false)` | 地図同期中 | Wi-Fi 待ち | |
| `ClientLink(N, false)` | 位置取り直し中、追跡中 | 変えない | 集計に数える（ホストの Wi-Fi から外れたクライアントが位置を出し続けるかは未確認なので、位置の行で判断する） |
| `Status(N, connected, map_ready)` | 接続、Wi-Fi 待ち、地図同期中（`connected` かつ `map_ready` のとき） | 位置取り直し中 | 初版の `Map(Synced か ReuseOk)` の行にあたる |
| `Status(N, ...)` | ほかの組み合わせ | 変えない | 数えない（状態の行は数分おきにも出る報告のため）。ただし、未接続以外の N に `connected: 0` が来たら「順序の合わない出来事」に数える（切断は `TrackerRemoved`、`DongleRemoved`、`ServerStarted`、`ServerStopped` だけで決める。U-5） |
| `Status(N, connected, usb = Some(v))` | 未接続以外（`link` が `Unknown` のとき） | 変えない | `link` を、v なら `Usb`、でなければ `Dongle`（途中から読み始めて、`TrackerAdded` を見ていない台のため）。`link` が決まっていれば変えない |
| `Status(N, connected, host = true)` | どれでも | 変えない | ホストの根拠（5.3）。どの状態の行でも、上の行と合わせて当てる |
| `Status(N, connected, battery = Some(b))` | 未接続以外 | 変えない | N の `battery` を b にする（W-11）。`battery` が `None` の行では変えない（前の値を残す） |
| `Pose(N, Ok)`、`Oet(N, RelocBack)` | 未接続以外 | 追跡中 | `tracked_since_connect = true` |
| `Pose(N, Lost か RecentlyLost)`、`Oet(N, LostTracking)` | 追跡中、地図同期中 | 位置取り直し中 | |
| `Pose(N, Lost か RecentlyLost か Other)` | 接続、Wi-Fi 待ち | 変えない | 数えない（接続の直後に、意味のなさそうな値や `status: 3` が出るのはふつう。メモの 3.1） |
| `Pose(N, Other)`、`Oet(N, Overspeed)` | そのほか | 変えない | 集計に数える |
| `PowerOffNotice(N)` | — | 変えない | R4 が振り返って使う（6.2）。状態は、そのあとの `TrackerRemoved` で変わる |
| `PcCommand` | — | 変えない | R4 が振り返って使う |
| `DongleRemoved` | 未接続以外で、`link` が `Usb` でない台 | 未接続 | 役割とシリアル番号をなしに、`link` を `Unknown`。ドングルが抜けると、ドングル経由の台の `Remove tracker` が出ないため（メモの 3.5。1 回の観察。U-12）。**`link` が `Usb` の台は変えない**（ドングルを通さない接続のため。シリアル番号と、集計の接続の時間も続く）。`link` が `Unknown` の台は、ドングル経由として切る（6 日分の切断の 109 件が、ドングル経由〔`connection type: 1`〕のため） |
| `ServerStarted` | どれでも | 全台を未接続 | 役割とシリアル番号をなしに、`link` を `Unknown`。PC の Wi-Fi 情報を `Missing`（5.2）、ホストをなしに（5.3）。終了の行がないまま起動し直すこともあるので（メモの 3.5）、ここで必ず捨てる |
| `ServerStopped` | どれでも | 全台を未接続 | 役割とシリアル番号をなしに、`link` を `Unknown`。PC の Wi-Fi 情報を `Unknown`、ホストをなしに（5.3） |

- 表にない組み合わせ（例: 未接続のトラッカーの `Pose`）は、状態を変えずに「順序の合わない出来事」として数える。ただし、`TrackerAdded` を一度も見ていない番号（途中から読み始めた）は例外で、その出来事から推し量った状態で行を足し、`inferred = true` にする（`Pose(Ok)` と `Oet(RelocBack)` なら追跡中、`Pose(Lost か RecentlyLost)` と `Oet(LostTracking)` なら位置取り直し中、`ClientLink(true)` なら地図同期中、`Status(connected, map_ready)` なら位置取り直し中、`Status(connected)` なら接続、など。推し量れない出来事なら「接続」。`Status` の `connected: 0` と `SerialKnown` では足さない）。推し量って足した行の `tracked_since_connect` は `false` から始める（追跡中で足したなら `true`）。`link` は、足した出来事が `Status` で `usb` があればその値、なければ `Unknown`（あとの `Status` で決まる）。`battery` も、足した出来事が `Status` で `battery` があればその値。推し量って足した行にも、新しい `conn` を付ける（下の「接続」）。
- `Status` の 5 つの行は、1 つの状態の行に、当てはまるものをすべて当てる（状態は上の 2 行のどちらかで決め、`link`、ホスト、電池は、それぞれの行で決める）。
- **電池**（`battery`。W-11）は、いまの接続のあいだの報告だけを持つ。N が未接続になるとき（`TrackerRemoved`、`DongleRemoved`、`ServerStarted`、`ServerStopped`）と、つなぎ直し（下の「重なり」）と、`TrackerAdded` で、なしにする（前の接続の値や、別の台の値を見せないため）。未接続の台の電池は、ログから分からない（7.4）。
- **VIVE Hub の番号**（`hub_number`。W-12。3.2）も、いまの接続のあいだの値だけを持ち、表でシリアル番号をなしにするところ（未接続になるとき、つなぎ直し、`TrackerAdded`）で、同じくなしにする。VIVE Hub は、つなぎ直すたびに番号を振り直しているように見える（U-18）ため。推し量って足した行は、なしから始める（`SerialKnown` で付く）。
- 同じ状態への遷移は、何もしない（`since` を変えない）。`Pose` と `OET` の両方の行が出ても、二重に数えない。
- **接続**（`conn`。R4〔6.2〕、ホストの根拠〔5.3〕、集計の時間〔8.1〕で共通に使う）: 1 つの番号 N で、未接続でなくなってから（`TrackerAdded`。推し量って足したときは、足した出来事の時刻）、未接続になるまで（`TrackerRemoved`、`DongleRemoved`、`ServerStarted`、`ServerStopped`）か、下の「重なり」のつなぎ直しまで。始まるたびに新しい `conn` を付ける（エンジンの中の通し番号。`rebuild` で振り直す）。同じ番号 N でも、次の接続は別の接続（番号はスロットで、つなぎ直すと別のトラッカーが使うため。3.2）。
- **重なり**（同じ出来事の行が 2 回出る。メモの 2.2）: 未接続以外の N に `TrackerAdded(N)` が来たとき、N の前の `TrackerAdded` から 1 秒（`state.dup_ms` = 1000。仮）以内なら、2 つ目の行として何もしない（数えない）。1 秒を超えていたら（前の `TrackerAdded` を見ていない、推し量って足した台も）、`TrackerRemoved` を見落とした**つなぎ直し**として、未接続を経ずに接続にし（上の表の 1 行目の「ほかに変えること」も行う）、「順序の合わない出来事」に数える。`TrackerRemoved` の重なりは、上の表の「未接続 → 変えない」で吸収する。どちらも、R4 と集計の `sessions` は、状態が変わった 1 つ目の行だけで数える。
- **つなぎ直しで接続を区切る**（レビュー〔2 回目〕の指摘）: つなぎ直しの `TrackerAdded(N)` の時刻を a とすると、**前の接続を a で終え、同じ a に新しい接続（新しい `conn`）を始める**。トラッカーごとの規則と集計では、N が a に未接続になり、同じ a に接続になったものとして扱う。
  - R4（6.2）: 新しい接続の長さ（条件 2）は a から数え、予告は a を境に前の接続と新しい接続に分ける（6.2 の「予告と接続の対応」）。前の接続の終わり a では R4 を判定しない（切断の行がなく、いつ切れたか分からないため）。
  - R2・R3・R5 の待ちとエピソードは、a の未接続で終わる（`Raised` のあとなら `Ended`）。新しい接続の待ちは、ふつうの接続と同じに始まる。
  - 集計（8.1）: 前の接続の時間は a まで、その接続で分かったシリアル番号の行に入る。新しい接続は a から。`sessions` に 1 足す。
  - ホストの根拠（5.3）: 前の接続のものになる（N の台を、そのままホストとして案内しない）。
  - R1 の列と、全台についての「未接続以外のトラッカーがなくなった」の条件には当てない（N は未接続を経ないため）。
  - **例**: A が番号 2 で 10 分つながり、`Remove tracker (2)` の行がないまま、a に `Add new tracker: (2), ...`（B）が来て、B が a + 3 秒に予告なしで切れる。B の接続は a から 3 秒なので、R4 の条件 2（30 秒以上）を満たさず、R4 は出ない（a より前の A の接続の長さを足さない）。B が a + 40 秒に切れれば、ほかの条件を満たすとき R4 が出る。`HostAssigned` の重なりは吸収しない（2 度目の命令は不具合の始まりの印で、捨てると R1 を見落とすため。重なっても、`Missing` のあとに `Got host wifi info.` が来れば `Held` に戻るので害はない）。
- **番号のない出来事**（`HostWifiObtained` と `WifiRequestUnanswered`。行に番号がない。メモの 3.1）:
  - `HostWifiObtained` は、PC の Wi-Fi 情報（5.2）だけを変える。どのホストの情報かは、そのときのホスト（5.3）で読む（Wi-Fi 情報の側には、ホストを持たない）。
  - `WifiRequestUnanswered` は、R1（6.2）が番号なしで数える（R1 が案内するのはホストで、尋ねたクライアントではないため）。状態の表では、尋ねたクライアントの候補（接続か Wi-Fi 待ちで、役割が `Host` でないトラッカー。ホストの役割は、5.3 で、命令でも報告でも `Host` になる）がちょうど 1 台なら、その台を Wi-Fi 待ちにし、役割を `Client` にする（画面に「Wi-Fi 待ち」と出すため。見本の 3 章では、2 度目の命令のあとにつながったトラッカー 0 が、これにあたる）。0 台か 2 台以上なら、状態を変えない（数えない。不具合のときは 1 日に数千回来るため）。
  - スレッド番号からトラッカーは分からない（同じスレッドが、ほかの台の行も書いている。見本の 3 章）。
- ホストの地図: ホストの状態の行が `map_ready` になるかは確かめていない（見本では、ホストの接続の直後の状態の行は `map status: -1`、`map state: 0`）。ホストは `HostAssigned` で地図同期中になり、`Status(map_ready)` か `Pose(Ok)` で先へ進む。

### 5.2 PC 側の Wi-Fi 情報

VIVE Hub のサーバーが、ホストの Wi-Fi 情報を持っているか。PC が実際に持っているかではなく、**ログから読み取れること**を表す。

| 出来事 | 次の状態 |
|---|---|
| （起動時） | `Unknown` |
| `ServerStarted` | `Missing { since: いま, queried: false }` |
| `HostAssigned(N)` | `Missing { since: いま, queried: false }`（前に `Held` でも。2 度目の命令で持たない状態になるのが不具合の始まり。`findings-2026-11-27.md`） |
| `HostWifiQueried(N)` | `Missing` なら `queried = true`。ほかは変えない（ホストの根拠としての扱いは 5.3） |
| `HostWifiObtained` | `Held { since: いま }`（どのホストの情報かは、5.3 のホストで読む） |
| `WifiAssigned` | `Held` でなければ `Held` にする（渡せたのだから持っている）。「順序の合わない出来事」として数える |
| `WifiRequestUnanswered` | 変えない（R1 が数える）。`Held` のときに出たら「順序の合わない出来事」として数える |
| `TrackerRemoved`（ホストが外れた）、`DongleRemoved` | 変えない（次のホストへの命令で `Missing` になる） |
| `ServerStopped` | `Unknown` |

- 初版の `Status(N, host = true)` の行（`Missing`・`Held` の `host` を埋める）は、5.3 に移した。Wi-Fi 情報が `Unknown` のとき（途中から読み始めたとき）にも、ホストを覚えるため。

### 5.3 ホストの根拠

どのトラッカーがホストか（`Snapshot::host`。3.5）。PC の Wi-Fi 情報（5.2）とは別に持つ。R1 の案内するトラッカー（6.2）、トラッカーの役割（下の注。番号のない問い合わせの候補を決める。5.1）、`HostWifiObtained` がどのホストの情報か（5.2）に使う。

根拠は 2 つの強さがある。**命令**（`HostBy::Command`）は `HostAssigned`（PC がそのトラッカーをホストにした行）。**報告**（`HostBy::Report`）は、状態の行の `is host: 1` と、`Query wifi from tracker N`（PC がホストに尋ねた行）。命令は報告より強い。

**根拠は接続に結びつける**（レビュー〔2 回目〕の指摘）。`HostRef` は、番号 `key` のほかに、根拠が来たときの N の接続 `conn`（5.1）と、その接続のシリアル番号 `serial`（分かれば）を持つ。番号が同じでも、別の接続の台はホストではない。ホストの `key` の行が未接続か、その行の `conn` が `HostRef::conn` と違えば、「**接続の終わったホスト**」と呼ぶ（3.5）。

| 出来事 | ホスト |
|---|---|
| （起動時。`rebuild` も） | なし |
| `HostAssigned(N)` | N のいまの接続（命令。`serial` は、その接続のシリアル番号）。前のホストが何でも変える。N が未接続なら（5.1 の表にない組み合わせ）、なしにする |
| `Status(N, connected = true, host = true)` | ホストがないか、報告のホストか、接続の終わったホストなら、N のいまの接続（報告）。接続の続いている命令のホストがあれば変えない（状態の行は命令のあとも少しのあいだ古い値を出しうるため。数えない） |
| `HostWifiQueried(N)` | ホストがないか、報告のホストか、接続の終わったホストなら、N のいまの接続（報告）。接続の続いている命令のホストがあれば変えない（それと違えば「順序の合わない出来事」に数える） |
| `SerialKnown(N, S)` | N のいまの接続がホストの接続なら、ホストの `serial` を S にする |
| `Status(N, connected = false, ...)` | 変えない（起動の直後に、前に登録したトラッカーの `connected: 0` の行が続く〔メモの 3.5〕。前の回の値を拾わないため） |
| `TrackerRemoved`、`DongleRemoved`、つなぎ直し（5.1） | 根拠（`key`、`conn`、`serial`）は残す。ホストの台が切れたら、接続の終わったホストになり、強さにかかわらず報告でも置き換わる（切れたあと、次の命令の前に R1 が出たとき、切れたホストを指せるように。R1 の案内は 6.2） |
| `ServerStarted`、`ServerStopped` | なし |

- ホストが N のいまの接続に変わったら、N の役割を `Host` にし、前のホストの接続が続いていれば（N でなければ）、その台の役割を `Unknown` にする。役割が `Host` のトラッカーは、多くて 1 台。ホストの台が切れると、5.1 で役割が `Unknown` に戻る。同じ番号に別のトラッカーがつながっても、別の接続なので、役割は `Unknown` から始まり、番号のない問い合わせの候補（5.1）に入る（ホストとして扱わない）。
- **例**（番号の使い回し）: `HostAssigned(2)` で A がホスト（命令）になり、PC の Wi-Fi 情報が `Missing` になる。A が `TrackerRemoved(2)` で切れ、別のトラッカー B が `TrackerAdded(2)` でつながる。ホストの根拠は A の接続のまま（接続の終わったホスト）で、B の役割は `Unknown`。B は問い合わせの候補に入る。このあと、次の命令の前に R1 が出ても、B をホストとして案内しない（6.2 の R1）。B の状態の行に `is host: 1` が来れば、ホストは B の接続（報告）に替わる。
- **例**（途中から読み始めた。前の日のファイルがなく、PC の Wi-Fi 情報は `Unknown`）: `Status(2, connected, host = true)`、`Status(4, connected, host = false)`、`Status(0, connected, host = false)`（どれも `TrackerAdded` を見ていないので、推し量って足す。5.1）。ホストは 2（報告）で、2 の役割は `Host`。続いて `WifiRequestUnanswered` が来ると、候補は 4 と 0 の 2 台なので、状態は変えない（5.1）。0 が `ClientLink(0, true)` で地図同期中になっていれば、候補は 4 だけで、4 が Wi-Fi 待ちになる。R1 が出れば、案内するのはトラッカー 2。

### 5.4 トラッカーの台帳と、画面に並べる台（W-11、W-12）

画面に並べるトラッカーの一覧の元（7.4）。#27 で足した。スロットの行（5.1）は、つなぎ直しで別の台に使われ、切れるとシリアル番号が外れる。そこで、**シリアル番号ごとに**、ログから分かったこと（VIVE Hub の番号、ファームウェアの版、最後に見た時刻）を台帳（`Identity` の並び。3.5）に持ち、切れても消さない（未接続の台も一覧に出すため。W-11 の接続の状態）。

| 出来事 | 台帳 |
|---|---|
| （起動時。`rebuild` も） | 空 |
| `SerialKnown(N, S, hub, fw)` | S の行がなければ足す。`firmware` は、fw が `Some` ならその値（`None` なら前の値のまま）。`last_seen` をいまに。VIVE Hub の番号（hub）は、台帳ではなく、N のスロットの行に付ける（5.1。接続ごとに変わるため） |
| `Status(N, connected = true, …)`（N の行にシリアル番号 S が付いている） | S の `last_seen` をいまに |
| `TrackerRemoved`、`DongleRemoved`、`ServerStarted`、`ServerStopped` | 変えない（台そのものの情報のため） |

- 持つのは 16 台まで（ドングルのスロットは 5 つ。ペアリングをし直した日には、別の台が増えうる）。17 台目を足すときは、いまつながっていない台のうち `last_seen` の最も古いものを消す（つながっている台は消さない。つながっていない台がなければ、`last_seen` の最も古い台を消す。スロットは 5 つなので、ふつうは起きない）。
- 台帳は、読み直し（起動、再構築）のたびに、前の日のファイルといまのファイルから組み立て直す（4.5）。そのため、一覧に出る未接続の台は、前の日かその日に一度でもつながった台。それより前にしか使っていない台は、つなぐまで出ない（VIVE Hub のペアリングの一覧はログから読めない。起動の直後の `connected: 0` の状態の行には、シリアル番号がない。メモの 3.5）。
- 台帳は、シリアル番号だけで台を見分ける。VIVE Hub の番号は、台に固定ではない（U-18。3.2）ので、台帳には持たない（画面に並べる台の `hub_number` は、スロットの行から取る。下）。

**画面に並べる台**（`Snapshot::devices`。`DeviceView`。3.5）は、台帳とスロットの行から、写しを作るたびに組み立てる（状態として持たない）。

- 台帳の台ごとに 1 つ。その台のシリアル番号が付いた、未接続以外のスロットの行があれば（5.1 で、1 つのシリアル番号は多くて 1 つの番号に付く）、その行の値を使う: `key` はその番号、`conn` はその行の `conn`、`hub_number` はその行の `hub_number`、`connection` はその行の `link`（`Usb` なら `Usb`、`Dongle` と `Unknown` なら `Dongle`。`Unknown` をドングル経由とするのは 5.1 の `DongleRemoved` と同じ理由）、`battery` と `phase` はその行の値、`host` は、その行がホストの接続（3.5 の「ホストの接続が続いている」）か。なければ、`key` なし、`conn` なし、`hub_number` なし、`connection = Disconnected`、`battery` なし、`phase = Disconnected`、`host = false`。
- **仮の台**: 未接続以外で、シリアル番号がまだ付いていないスロットの行ごとに 1 つ（`serial`、`hub_number`、`firmware` がない。`key` と `conn` はその行の値）。接続の直後（ふつう 1 秒以内に `SerialKnown` が来る。見本の 4 章）と、途中から読み始めて接続の始まりを見ていない台。`SerialKnown` が来たら、仮の台は消え、台帳の台の 1 つになる（同じ台が 2 つ並ばない）。
- 台数を決めない（W-9、W-12）。3 台でも 5 台でも、同じ決まりで並ぶ。

### 5.5 ドングル（W-11）

ドングルの接続（`Snapshot::dongle`）。#27 で足した。表示（7.4）だけに使い、規則とトラッカーの状態は変えない（ドングルが抜けたときにトラッカーを切るのは、これまでどおり `DongleRemoved` の 5.1 の行）。

| 出来事 | ドングル |
|---|---|
| （起動時。`rebuild` も） | `Unknown` |
| `DongleAdded` | `Connected` |
| `DongleRemoved` | `Disconnected` |
| `DongleReport(connected)` | connected なら `Connected`、でなければ `Disconnected`（報告。途中から読み始めて、`Dongle is added.` を見ていないときのため） |
| `TrackerAdded(N, usb = false)` | `Connected`（ドングルを通してつながったのだから、ドングルはある。前が `Disconnected` でも） |
| `ServerStarted` | `Unknown`（ドングルがあれば、すぐあとに `Dongle is added.` が来る。見本の 1 章で 0.4 秒後） |
| `ServerStopped` | `Unknown`（VIVE Hub が止まると、ログからは分からないため） |

- `DongleReport` と `DongleAdded` の行が、ドングルがないときにどう出るかは確かめていない（U-20）。

### 5.6 VIVE Hub のトラッカー用サーバー（W-10）

VIVE Hub のサーバーが動いているか（`Snapshot::hub`）。手引きの最初の段（7.4 の G1）と、利用者に見せる全体の状態（7.4）に使う。#27 で足した。

| 出来事 | `HubState` |
|---|---|
| （起動時。`rebuild` も） | `Unknown` |
| `ServerStarted` | `Running` |
| `ServerStopped` | `Stopped` |

- ほかの記録では変えない（`Release begin` のあとの終了の途中の行で、`Running` に戻さないため）。`Unknown` は、行は読めているが、開始と終了の行を見ていない（途中から読み始めた）ことで、動いているものとして扱う（7.4）。
- **ログだけでは足りない**: 終了の行のないまま止まることがある（6 日で 4 回。PC の終了や強制の終了。メモの 3.5）。そのときログは `Running` のまま伸びなくなるが、トラッカーがつながっていないと、動いている VIVE Hub も何時間も行を書かない（最長 10.6 時間。メモの 3.5）ので、伸びないことからは見分けられない。
- そこで、**アプリ（Windows）は、プロセスの一覧を見て、`ViveTrackerServer.exe` があるかを確かめる**（M1-9）。確かめるのは、起動したとき（ログを読み始める前）に 1 回、そのあと 10 秒ごと（`watch.process_check_s`。仮）と、下の「確かめ直し」のとき。読むだけで、プロセスにも機器にも何も送らない（W-7。11 章）。名前は大文字と小文字を区別せずに比べ、設定で足せる（`m2-dongle-reader.md` の DG-3 の確かめと同じ決まり。名前の根拠も同じ）。クレートは M2 と同じ `sysinfo`（0.40.4、MIT。`default-features = false, features = ["system"]`。D-32、`m2-dongle-reader.md` の 10 章）。判定（名前の一覧 → あるか）は、入出力のない関数にする。
- **止まっているあいだも、エンジンの時刻は進む**: 見張りの時刻は壁時計で進む（4.6）ので、止まる前に始まった待ちの期限（R2 の 30 秒、R1 の `Escalated` など）は、止まっているあいだに判定される。再生でも、次の行（起動し直したときの開始の行）の時刻で同じ期限が判定されるので、`Detection` は同じ（D-22）。そのため、**古い値を見せない・案内を出さない、は、画面と通知の出口で行う**（下の「出口の見張り」。PR #37 のレビュー〔2 回目〕の指摘）。

**出口の見張り**（`openut_core::display::HubGate`。入出力なし。PR #37 のレビュー〔2 回目〕の指摘で足した）: プロセスの確かめと、エンジンに渡した `ServerStarted` から、**ログから組み立てた値が、いま動いている VIVE Hub のものか**を決める。表示の写し（7.4）と配信の決まり（7.1）が、同じ答えを使う。アプリ（M1-9）が持ち、確かめの結果と、渡した出来事を知らせる。

```rust
/// Whether the values built from the log are about the VIVE Hub that runs now.
pub enum HubLive {
    Unknown,   // no process check (Linux, replay), or the checks are not trusted (below)
    Running,   // found, and the log shows the session that runs now
    Stopped,   // not found
    Starting,  // found, but the start of its new session has not been given to the engine yet
}
/// 止まっている・起動の途中: the values of the log are not about a running VIVE Hub (7.1, 7.4).
pub fn hub_down(live: HubLive, log: HubState) -> bool;
// = live is Stopped or Starting, or live is Unknown and log is Stopped
```

`HubGate` が覚えるのは、最後の確かめの結果（ある・ない・確かめていない）、「**開始待ち**」（止まったと確かめたあと、新しいセッションの開始の行をまだ渡していない）、「**止まった印**」（止まった VIVE Hub が書いた記録の、最も新しい時刻。下）、`Stopped` と `Starting` になった壁時計の時刻、確かめを信じるか、の 5 つ。

| 入力 | `HubGate` |
|---|---|
| （起動時） | 確かめていない。開始待ちなし。止まった印なし |
| 確かめ: ない（確かめ直しも） | 開始待ちを立てる。止まった印を、それまでに読んだサーバーの記録の最も新しい時刻まで上げる |
| 確かめ: ある | 変えない |
| サーバーの記録を読んだ（起動したときの確かめが「ない」で、起動した時点でファイルにあった記録） | 止まった印を、その記録の時刻まで上げる |
| `ServerStarted` をエンジンに渡した（渡した直後。**追いつきの中でも外でも**） | その時刻（エンジンが使う時刻。4.6 で時刻が戻る行は、いまの時刻）が止まった印より後なら、開始待ちを下ろす。止まった印と同じか前なら変えない（止まる前のセッションの開始の行）。止まった印がなければ下ろす |
| 再構築（4.1） | **変えない**（開始待ちも、止まった印も保つ） |

止まった印は下げない（アプリを終えるまで）。答え（`HubLive`）は、上から順に、最初に当てはまるもの: 確かめを信じない、か、確かめていない → `Unknown`。最後の確かめが「ない」→ `Stopped`。開始待ちが立っているか、ログが `Stopped`（`HubState`。終了の行があって止まり、起動し直した直後）→ `Starting`。ほか → `Running`。

- **止まった印**（PR #37 のレビュー〔3 回目〕の指摘で、前の版の「追いつきの外の開始の行だけを数える」を替えた）: 確かめが「ない」なら、その確かめより前にファイルに書かれていた記録は、止まった VIVE Hub のもの（止まったプロセスは書けず、起動し直した VIVE Hub の記録は、その確かめより後に足される）。確かめより前に読んだ記録は、確かめより前に書かれていたので、止まった印に入れる。起動したときは、ファイルの一覧（大きさ）を見てから確かめる（M1-9）。確かめが「ない」なら、その一覧の時点でファイルにあった記録（4.5 の 1 と 2 の、読み終えまで）も、読むたびに入れる。起動し直した VIVE Hub の開始の行は、同じ PC の時計で、止まった VIVE Hub のどの記録よりも後の時刻になる。そのため、**時刻だけで、いまのセッションの開始の行かを決められ**、ファイルの位置が分からなくなる再構築（4.1）をまたいでも、同じ決まりで見分けられる。
- **改めた理由**（PR #37 のレビュー〔3 回目〕）: 前の版は、再構築では、最後の確かめが「ある」なら開始待ちを下ろしていた。終了の行のないまま止まり、プロセスだけが戻った起動の途中に、ドライバーのファイルだけが作り直されると、両方を読み直す（4.1）が、サーバーのファイルは止まる前のままなので、追いつきが終わると止まる前の追跡の状態が戻り、出していなかった案内も追いつきの終わりに出ていた。この版は、再構築で開始待ちを変えず、止まった印より後の開始の行でだけ下ろす。前の版の「追いつきの外の開始の行だけを数える」では、起動したときの追いつきのあいだに VIVE Hub が起動して書いた開始の行を、追いつきの中で読むと数えられず、起動の途中のまま止まっていた。この版は、それも数える（起動した時点のファイルより後に書かれ、止まった印より後の時刻のため）。
- **再構築のとき**: 開始待ちと止まった印を保つ。再構築の追いつきで読み直す記録は、止まった印までなら止まる前のセッションのもので、開始の行があっても開始待ちを下ろさない。写しは、追いつきのあいだも、終わってからも、起動の途中（7.4 の全体の 5）か、止まっている（1）で見せ、追いつきの終わりにも push しない（7.1）。作り直したファイルに、起動し直した VIVE Hub の開始の行がすでにあれば（止まった印より後の時刻）、追いつきの中で開始待ちを下ろし、追いつきの終わりには、新しいセッションの値をふつうに見せて、続いている案内をふつうに出す。
- **確かめの合間に起動し直したとき**: 確かめは 10 秒おきなので、起動し直した VIVE Hub の記録を、次の確かめより前に読むことがある。そのときは、下の「確かめ直し」で、記録をエンジンに渡す前に「ある」になるので、開始の行で開始待ちを下ろせる（`Starting` に止まり続けない）。
- **確かめ直し**: `Stopped` のあいだに、時刻が止まった印より後のサーバーの記録を読んだら（追いつきの中でも外でも）、エンジンに渡す前に、その回のうちにもう一度確かめる（起動し直したなら見つかり、開始の行を渡す前に `Starting` になる）。見つからなければ、止まった印がその記録まで上がる（上の表。止まったプロセスの最後の書き込みを、確かめのあとに読んだもの）ので、その中の開始の行では下ろさない。それでも見つからず、`Stopped` になってから `watch.process_check_s` より後なら、**名前が合っていない**（VIVE Hub が、設定にない名前で動いている。止まったプロセスの最後の書き込みは、止まってから 1 回の読み取り〔500 ms〕のうちに読み終えている）とみなし、アプリを終えるまで確かめを信じない（`Unknown`。ログだけで決める初版の動きに戻る。アプリのログに 1 回書く）。名前が違うまま止まり続けて、画面が G1 のままになり、案内も出なくなることを防ぐため。
- **残る食い違い**: 止まったプロセスの最後の書き込み（止まる直前の 1 回の書き込み）を、確かめのあとに読み、確かめ直しの時にはもう起動し直していた（10 秒のうちに、止まって起動し直した）ときだけ、その書き込みは止まった印に入らない。その中に開始の行があれば（起動してすぐに止まったセッション）、それで開始待ちを下ろすが、開始の行のあとの状態は全台が未接続から始まる（5.1）ので、止まる前の値は戻らず、起動し直した VIVE Hub の開始の行で、もう一度捨てられる。PC の時計が戻されて、再構築の追いつきの中の新しい開始の行が止まった印と同じか前の時刻になったときは、開始待ちを下ろせず、下の「開始の行が来ないとき」になる（OpenUT を起動し直せば、起動したときの確かめで見つかるので戻る）。
- **開始の行が来ないとき**: `Starting` が壁時計で `watch.start_wait_s`（仮 60 秒）を超えたら、見張り役の具合を `FormatMismatch` にする（7.4。VIVE Hub は動いているのに、開始の行を読めない。版が変わって綴りが変わったか、書くファイルが変わった見込み。理由はアプリのログ）。`Starting` のまま（値を戻さず、案内を出さない）。開始の行を渡したら、この理由の `FormatMismatch` は解く。決め方: VIVE Hub は、起動したときに `# Log begin at` と開始の行を同じミリ秒に書き（見本の 1 章）、まとめて書く間隔は 5 秒（4.6）なので、ふつうは数秒で `Running` に戻る見込み。M1-9 が、プロセスが見つかってから開始の行を渡すまでの壁時計の時間をアプリのログに残し、オーナーの試し（14 章の 7）と合わせて、その最大値に余裕を足した値にする。
- `Unknown`（Linux、再生、確かめを信じないとき）は、ログだけで決める: `HubState` が `Stopped` なら止まっている（7.4 の「全体」の 4）。終了の行のないまま止まったことは、ログが伸びないこと（`Stalled`。7.4）で、10 分あとに分かる。

**止まっている・起動の途中のあいだ**（`hub_down` が `true`）に、それぞれの出口がすること:

- **画面**（7.4）: 手引きの G1 を見張り役の具合（`Stalled` など）より先に出すか（`Stopped`。追いつきのあいだも）、何もしないで待つか（`Starting`。再構築の追いつきのあいだも）で、台をすべて未接続として見せ、案内を欄にも枠にも出さない（7.4 の「全体」の 1・4・5、「止まったときの見せ方」）。**起動し直して、プロセスが見つかっても、開始の行を渡すまでは、止まる前の値（接続、追跡の状態、ホスト、電池、R1 などの案内）を、いまの値として戻さない**。開始の行で、状態の写しは全台が未接続から始まる（5.1）ので、そのあとはふつうに見せる。
- **通知**（7.1）: アプリが `Engine::set_hub_down(true)` を渡し、配信の決まりは push しない（止まっているあいだに期限が来た案内と、待たせていた案内も出さない。まだ `drain` で渡していない push も取り消す）。`HubState` を変える出来事（開始の行と終了の行）を渡した直後に、次の出来事を渡す前に、その時の答えで `set_hub_down` を渡し直す（開始の行のあとは、ふつう `false`。終了の行のあとは `true`。M1-9 の順）。**終了の行を読んだ回は、その行から push しない**（PR #37 のレビュー〔3 回目〕の指摘。次の確かめを待たない。そのあとの終了の途中の行で時刻が進み、待たせていた push の間隔が過ぎても出さず、その回のそれより前の行で作った push も、`drain` の前に取り消される）。再構築のあとも渡し直す（開始待ちが立っていれば `true` のまま）。
- **見張り役の具合**（7.4）: `Stopped` と `Starting` のあいだは、`Stalled` にしない（止まったことはプロセスで分かっていて、画面は止まったときの見せ方になるため。起動し直した直後に「VIVE Hub を起動し直す」と出さないため）。
- 状態の写しそのもの（エンジン）、規則、集計は変えない（D-22）。プロセスの確かめは、規則と集計には使わない（ログの時刻だけで動かす決まり）。再生（10.2）と `openut replay` は、出口の見張りを使わない（確かめていない。`Unknown`）。

## 6. 検出の規則（W-3、W-4）

### 6.1 共通の決まり

- 規則は、状態と出来事と時刻から `Detection` を出す（入出力なし）。1 回の「条件が成り立ってから、なくなるまで」を**エピソード**と呼び、番号をつける。エピソードは、`Raised`（始まり）→ 任意で `Escalated`（長引いた）→ `Resolved`（直った）か `Ended`（直ったとは言えないが、条件がなくなった）の順に出る。
- エピソードのあいだ、同じ規則・同じトラッカーの `Raised` は出さない。案内を何回出すか（`push`）は、7.1 の配信の決まりが決める。
- 規則の判定に使うのは、ログの時刻だけ。
- **時間の境目**（すべての規則で同じ。3.7 の順の決まりで実現する）:
  - 「X 秒以内」「前後 X 秒」は、ちょうど X 秒を含む。「X 秒を超えて」は含まない。
  - 「起点から X 秒たっても〜しない」の期限は、起点 + X 秒。期限ちょうどの時刻に「〜した」入力があれば、間に合ったものとして `Raised` を出さない。期限を 1 ms でも過ぎた入力は間に合わない。出した `Raised` の `at` は期限の時刻。
  - 回数で決まる条件（R1）は、その入力を適用したときに判定し、`at` はその入力の時刻。
- **しきい値はすべて仮の値**。決め方:
  1. 匿名化した実物のログ（L-2。PM が `tests/` に入れる。サーバーとドライバーのファイル。2026-11-22 のファイルはもう残っていない〔メモの 1 章〕ので、11/23〜27 の見込み。10.4）を、`openut replay` で、しきい値を変えながら流す（`--set` で上書き。実装計画の M1-9）。
  2. R1 は、受け入れの基準（4 回だけ。10.4）を満たす範囲を探し、その範囲の真ん中寄りの値にする。R2〜R5 は、`findings-2026-11-27.md` の数字（喪失 234 回、1 分を超える喪失 19 回、地図の受け取り 1〜12 秒と 40〜550 秒、操作と関係のない切断 1 日 1〜3 回）と、出る案内の数を見比べて、1 晩に出る案内が数回に収まる値にする。
  3. 決めた値と、そのときの検出の数を、実装の PR の説明に書く。値を変えるのは設計の変更なので、PM に報告する（設計の追補で、この表を直す）。
- しきい値の名前は、設定ファイル（9.1）と `--set` と `expect.toml` で共通。`--set` では表の名前から書く（`rules.r1.count`、`summary.gap_s` など。9.1）。この章では、`rules.` を省いて `r1.count` のように書く。

### 6.2 規則

#### R1 Wi-Fi 情報なし（W-3）

- **根拠**: `findings-2026-11-27.md` の「始まらない問題」。4 回とも、`switch to HOST mode` が 2 度出て、2 度目のあとに `Query wifi` と `Got host wifi info` が出ず、以後 `Wifi info is not yet available.` が続き、後から来たトラッカーに `Assign wifi info` が一度も出なかった。メモと見本で、次が分かった。
  - `Wifi info is not yet available.` と `Got host wifi info.` には、トラッカーの番号がない（メモの 3.1）。R1 は、番号なしで数える（尋ねたクライアントではなく、ホストを案内するため。5.1 の「番号のない出来事」）。
  - 出るときは続けて出る。間隔の中央値は 0.37 秒、9 割が 0.56 秒以内。6 日のうち 4 日は 0 件で、出た 2 日は 11/26 01:43〜01:52 に 2,071 件、11/27 22:53〜23:01 に 671 件（メモの 3.1）。ふつうの開始では、この 6 日分には出ていない。
  - 見本の 3 章（11/27 22:53）: ホスト 2 の `Got host wifi info.` のあと、22:53:32.961 に 2 度目の `Tracker (2) - switch to HOST mode.`、0.3 秒後にトラッカー 0 がつながり、22:53:33.658 から問い合わせが続く。最初の 1 件から 15 秒の前後に、ほかの種類の記録はない。
- **断られた問い合わせの列**: 続けて来た `WifiRequestUnanswered` のまとまり。R1 は、いまの列の回数と、列の 1 回目の時刻だけを持つ（それより前の問い合わせは覚えない）。
  - 前の `WifiRequestUnanswered` から `r1.gap_s`（仮 10 秒）以内に来たものは、同じ列に足す。`r1.gap_s` を超えて空いたら、その問い合わせから新しい列を始める（回数を 1、1 回目の時刻をその行の時刻にする）。不具合のときは 1 秒に満たない間隔で続く（根拠）ので、列は切れない。ふつうの開始で、間を空けて何回か断られても、1 つの列にはならない。
  - 次のときは列を捨てる（次の問い合わせから新しい列を始める）: `HostAssigned`、`HostWifiObtained`、`WifiAssigned`、`ServerStarted`、`ServerStopped`、`DongleRemoved` が来たとき（`DongleRemoved` は、USB の台が残っていても。5.1）。未接続以外のトラッカーがなくなったとき。R1 のエピソードが終わったとき（`Resolved`、`Ended`）。
- **条件**: PC の Wi-Fi 情報が `Held` でなく、いまの列の回数が `r1.count` 以上で、列の 1 回目から `r1.span_s` 秒以上たった `WifiRequestUnanswered` が来たとき、`Raised`（時刻はその行の時刻）。エピソードのあいだは、列を数えない。
  - 仮の値: `r1.count = 3`、`r1.span_s = 15`、`r1.gap_s = 10`（変えない）。ふつうの開始でも、ホストが Wi-Fi の親機を立てるまでのあいだに、クライアントが尋ねて断られることがありうる（6 日分には出ていないが、ほかの環境では分からない）。`span_s` は、それを不具合と見なさないための余裕。`gap_s` は、断られる間隔（9 割が 0.56 秒以内）より十分長く、離れた問い合わせを 1 つに数えない長さ。決め方は 6.1。`gap_s` は `quiet_s` より短くする（始まりは厳しく、終わりはゆるく）。
  - `Query wifi from tracker N` が出たかどうか（`queried`）は条件に入れない。綴りは分かった（4.4）が、尋ねたのに答えが来ない場合も、利用者がすること（ホストの電源の入れ直し）は同じため。`Detection` の詳細として残す。
- **案内するトラッカー**: ホスト（5.3）。ふつうは最後に `HostAssigned` された台。途中から読み始めて `HostAssigned` を見ていなければ、状態の行で `is host: 1` だった台（PC の Wi-Fi 情報が `Unknown` のままでも分かる）。**ホストの根拠は接続に結びついている**（5.3）ので、案内する台（`{H}`）は、案内を作るたび（`Raised`、`Escalated`）に、次の順で決める。
  1. ホストの接続が続いていれば、その台（番号 `key`）。呼び名は、その番号の行のシリアル番号（末尾の長さは台帳〔5.4〕で決める）と、その行の VIVE Hub の番号（いまの接続）で作る（3.2）。シリアル番号がまだ付いていなければ、番号のない呼び名（「ホストのトラッカー」。3.2 の表）。
  2. 接続の終わったホストで、根拠の `serial` が分かっていれば、同じシリアル番号の台を探す。いまどこかの番号 M でつながっていれば、その台（番号 M。つなぎ直して番号が変わった同じトラッカー）。つながっていなければ、台帳のその台の呼び名（末尾だけ。つながっていない台には番号を付けない。3.2。同じトラッカーと分かるため。初版の「ニックネームがあれば」は、ニックネームをやめたので、これに替えた）。
  3. どれでもなければ（ホストがない、接続の終わったホストのシリアル番号が分からない）、「ホストのトラッカー」。**同じ番号にいまつながっている別の台の呼び名は使わない**（レビュー〔2 回目〕の指摘。番号の使い回しで、別の台の電源を案内しないため）。
  - `Detection` の `tracker`（3.6）は、1 か 2 で台が決まり、いまつながっているときはその番号（1 は `key`、2 は M）、それ以外は `None`。`serial` は、1 ではその番号に付いたシリアル番号（なければ `None`）、2 では根拠の `serial`（つながっていなくても）、3 では `None`。`conn`（3.6。PR #37 のレビューの指摘で足した）は、1 ではホストの接続（`HostRef::conn`）、2 でいま M でつながっていれば M のいまの接続、それ以外は `None`。表示の写しが、シリアル番号の来る前に出した案内を、同じ接続の台に当てるため（7.4 の「案内の当て方」）。`Escalated` でも、案内する台を決め直したときの値にする（3.6）。`expect.toml` の `tracker`（10.3）は、`tracker` の値と比べる（`conn` は比べない）。
  - **例**（5.3 の例の続き。A のシリアル番号は分からない）: B が番号 2 でつながっているあいだに R1 が出ると、案内は「ホストのトラッカーの電源だけを入れ直す」で、`tracker` も `serial` も `None`。A のシリアル番号が分かっていれば（台帳にある）、A の呼び名（例: `トラッカー（…0001）`。つながっていないので末尾だけ）で、`tracker` は `None`、`serial` は A のシリアル番号、`conn` は `None`。A が番号 4 でつなぎ直していれば（同じシリアル番号の `SerialKnown(4, …)`）、A の呼び名（例: `トラッカー 2（…0001）`。いまの接続の番号）で、`tracker` は 4、`conn` は番号 4 のいまの接続。
- **長引いたとき**: `Raised` から `r1.escalate_s`（仮 180 秒）たっても続いていたら、`Escalated` を 1 回だけ出す（期限は `Raised` の時刻 + `r1.escalate_s`。ちょうどに直れば出さない）。
- **直った**: `HostWifiObtained` か `WifiAssigned` が来たら `Resolved`。
- **終わった**: 未接続以外のトラッカーがなくなったとき、`DongleRemoved` が来たとき（USB の台が残っていても。ドングル経由の台がなくなれば、トラッカー同士の Wi-Fi の話でなくなるため）、または `WifiRequestUnanswered` が `r1.quiet_s`（仮 60 秒）を超えて来なかったとき（期限は最後の問い合わせの時刻 + `r1.quiet_s`）、`Ended`。
- エピソードのあいだに次の `HostAssigned` が来ても、エピソードを続ける（ホストの電源を入れ直すと、まず `HostAssigned` が来て、そのあとに `HostWifiObtained` が来ると見込むため。見込みは未確認）。列は捨てるが、エピソードの `Ended` の期限は、最後の問い合わせの時刻のまま数える。
- **例**（どれも同じホストで、Wi-Fi 情報は `Missing`）:

  | 経過 | 結果 |
  |---|---|
  | 断られた問い合わせが 0 秒と 0.5 秒に 2 回、61 秒あとに 1 回 | 出ない（61 秒の空きで列が切れ、3 回目は新しい列の 1 回目） |
  | 0 秒から 0.55 秒おきに 60 秒続く | 15.4 秒の問い合わせ（列の 1 回目から 15 秒以上たった最初のもの）で `Raised` が 1 回。最後の問い合わせから 60 秒を超えたところで `Ended` |
  | 見本の 3 章（22:53:33.658 から） | 22:53:48.658 以後の最初の問い合わせ（見本では 22:53:48.721）で `Raised`。案内するのはトラッカー 2 |
  | 上の `Ended` のあと、問い合わせが 1 回 | 出ない（`Ended` で列を捨てたので、新しい列の 1 回目） |
  | 上の `Ended` のあと、また 0.55 秒おきに 15 秒以上続く | 新しいエピソードとして `Raised` が 1 回 |
- **電源を入れ直すトラッカーがホストであること**の効果は、まだ確かめていない（`findings-2026-11-27.md` の「次に確かめること」の「ホストの入れ直しで不具合が直るか」）。案内の文言はこの見込みに基づく。オーナーに試してもらう手順の案は 14 章。

#### R2 地図同期が遅い（W-4）

- **根拠**: クライアントが地図を受け取るまで 1〜12 秒の回と 40〜550 秒の回がある（`findings-2026-11-27.md`）。
- **条件**: クライアント N が `ClientLink(N, true)` で地図同期中になってから `r2.wait_s`（仮 30 秒）たっても地図同期中のままなら、`Raised`（期限は `ClientLink` の時刻 + `r2.wait_s`。期限ちょうどに地図を受け取って位置取り直し中になれば出さない。6.1）。
- **直った**: N が位置取り直し中か追跡中になったら `Resolved`。**終わった**: N が未接続か Wi-Fi 待ちになったら `Ended`。
- 案内の「ホストの近くに置く」は、地図がトラッカー同士の Wi-Fi で渡ることからの推定で、効果は確かめていない。

#### R3 位置合わせが終わらない（W-4 の「ホストが位置を出せない」を含む）

- **根拠**: ホストが位置を出すまで 5.5〜322 秒（`findings-2026-11-27.md`）。SLAM の開始から最初の位置まで 21〜62 秒。静止したまま起動すると 7 分 45 秒失敗し続けた回がある（`findings-2026-11-28.md`）。
- **対象**: **既定ではホストだけ**（W-4 の「ホストが位置を出せない」のとおり）。設定 `rules.r3.clients = true` で、クライアントにも同じ規則を当てる（位置合わせの待ちはどのトラッカーにも起き、案内も同じため）。クライアントに当ててよいかは PM の判断を待っている（14 章）。認められたら、既定を `true` に変える（設計の追補と、実装計画の 3 章の後続の修正）。推し量って足したトラッカー（`inferred`。5.1）には当てない（接続の始まりを見ていないため）。
- **条件**: 接続してから一度も追跡中になっていない（`tracked_since_connect = false`）対象のトラッカーが、「位置合わせを始めた時刻」から `r3.wait_s`（仮 60 秒）たっても追跡中にならなければ、`Raised`（期限は始めた時刻 + `r3.wait_s`。期限ちょうどに追跡中になれば出さない。6.1）。位置合わせを始めた時刻は、ホストなら `HostAssigned` の時刻、クライアントなら位置取り直し中になった時刻。
  - 初版の案の 30 秒は、最初の位置まで 21〜62 秒かかることと合わない（ほとんど毎回出る）ので、60 秒にした。
- **直った**: 追跡中になったら `Resolved`。**終わった**: 未接続になったら `Ended`。`rules.r3.clients = false` のときは、ほかのトラッカーへの `HostAssigned` でホストでなくなったときも、待ちをやめる（`Raised` のあとなら `Ended`）。

#### R4 操作と関係のない切断（W-4）

- **根拠**: 操作と関係のない切断は 1 日 1〜3 回（`findings-2026-11-27.md`）。SLAM のクラッシュで接続ごと切れた例（11/23 22:38。`findings-2026-11-28.md`）。メモの 3.5: サーバーの `Remove tracker` は 6 日で 128 件（同じ出来事で 2 行ずつ出る）。そのうち 86 件は、ドライバーのファイルの電源オフの予告（`(N) Tracker is going to power-off.`）が先に出ていた（予告は 44 件）。予告のない 42 件のうち 19 件は、USB でつないでいたもの（`connection type: 2`。推定）。
- **条件**: `TrackerRemoved(N)` が時刻 t に来て、N の状態が未接続に変わったとき（2 つ目の行では見ない。5.1 の重なり）、次のすべてを満たせば `Raised`。幅はどれも両端を含む（6.1）。
  1. 直前 `r4.cmd_window_s`（仮 12 秒。`findings-2026-11-27.md` で喪失と命令の関係を見たときの幅）以内（t − 12 秒〜t。t と同じ時刻で、切断の行より後に書かれたものも含む）に、PC から N への命令（`HostAssigned`、`HostWifiQueried`、`WifiAssigned`、`PcCommand`。`PcCommand` の番号なしは、全台への命令。4.4）がない。
  2. 切れた接続（下の「接続」）が `r4.min_connected_s`（仮 30 秒）以上続いていた（開始のときのつなぎ直しを除く）。
  3. ほかのトラッカーの `TrackerRemoved` が前後 `r4.burst_s`（仮 2 秒）以内（t − 2 秒〜t + 2 秒）にない（全台が同時に外れるのは、ドングルの抜き差しやリセット、サーバーの終了と見なす）。
  4. 前後 `r4.server_s`（仮 5 秒）以内（t − 5 秒〜t + 5 秒）に `ServerStarted`、`ServerStopped`、`DongleRemoved` がない（メモの 3.5。サーバーの終了の行と、ドングルが抜けた行）。
  5. **電源オフの予告がない**: `rules.r4.exclude_power_off = true`（既定。仮）のとき、**切れた接続の予告**（下の「予告と接続の対応」）が、t − `r4.notice_before_s`（仮 15 秒。メモで予告と切断を突き合わせた幅）〜t + `r4.notice_after_s`（仮 1 秒）にない。予告のあった切断は、利用者がボタンで電源を切ったものと見込んで除く（下の注）。
  6. **USB の切断でない**: `TrackerRemoved` の `usb` が `false`（ケーブルを抜いたのは利用者の操作と見なす）。
- **接続**: 5.1 の「接続」（`conn`）。1 つの番号 N で、状態が接続に変わってから（`TrackerAdded`。推し量って足したときは、最初に見た時刻）、未接続に変わるまで（`TrackerRemoved`、`DongleRemoved`、`ServerStarted`、`ServerStopped`）か、**切断の行を見落としたつなぎ直し**（5.1 の「重なり」で、1 秒を超えた次の `TrackerAdded(N)`）まで。つなぎ直しの時刻 a で前の接続が終わり、同じ a に次の接続が始まる（5.1 の「つなぎ直しで接続を区切る」）。前の接続の終わり a では、切断の行がないので R4 を判定しない。同じ番号でも、次の接続は別の接続（番号はスロットで、つなぎ直すと別のトラッカーが使うため。3.2）。条件 2 の長さは、切れた接続の始まり（つなぎ直しなら a）から t まで。
- **予告と接続の対応**（ログの時刻で決める。届いた順では決めない。4.6）: 予告 `PowerOffNotice(N)` の時刻を u とする。
  - u のときに N の接続が続いていれば（始まり ≤ u ≤ 終わり。まだ終わっていなければ、始まり ≤ u）、その接続の予告。
  - u のときに N の接続がなければ、u の直前に終わった N の接続の予告。ただし、終わりから `r4.notice_after_s` 以内のときだけ（それより後なら、どの接続の予告でもない）。
  - u が、ある接続の終わりと、次の接続の始まりの両方と同じ時刻なら、終わったほうの予告（予告は、切断より先か同じミリ秒に出るため。メモの 3.5、見本の 5 章）。つなぎ直し（5.1）の a でも同じで、a より前と a ちょうどの予告は前の接続のもの、a より後の予告は新しい接続のもの。
  - このため、同じ番号の次の接続の予告で、前の接続の切断を除かない。前の接続の予告で、次の接続の切断を除くこともない。**例**: A が t に切れ、B が t + 3 秒に同じ番号でつながり、t + 8 秒に B の予告が出る → B の予告は B のもので、A の R4 は出る（`r4.notice_after_s` を 10 秒に広げても同じ）。A の予告（時刻 t）が、B の `TrackerAdded` より後に届いても、時刻 t で対応を決めるので、A の切断は除かれる。
  - エンジンは、判定を済ませていない切断について、その接続の始まりと終わりと、同じ番号の次の接続の始まり（あれば）を覚えておく（予告が遅れて届いても対応を決められるように）。
- **後ろの幅を 1 秒にした理由**: 予告は、切断の行より先か、同じミリ秒に出ていた（メモの 3.5 は「予告のあと 15 秒以内に切断」の形で 86 件を数え、予告 44 件のほとんどが対応した。見本の 5 章は同じミリ秒）。後ろの幅は、2 つのファイルの時刻の付け方の揺れの分だけにする。初版の `r4.notice_wait_s`（10 秒）は、ドライバーのファイルが遅れて届くことへの余裕を、ログの時刻の幅で取っていたもので、これをやめた。**届く遅れは、4.6 の「ドライバーを待つ」で別に扱う**（ログの上の照合の範囲と、追記を待つ時間を分けた）。決め方: 匿名化した実物のログ（L-2）で、予告の時刻 − 切断の時刻の分布を数える（M1-13 の PR に書く）。切断より後に出る予告があれば、その最大値に余裕を足す。
- **判定の期限**: 3〜5 で切断のあとの数秒を見るため、t + max(`r4.burst_s`, `r4.server_s`, `r4.notice_after_s`)（仮 5 秒）にして、期限を過ぎてから条件を確かめる。期限ちょうどの時刻の `ServerStarted` や切断も、先に適用されるので、条件に入る（3.7）。`Raised` の `at` は期限の時刻。条件 5 を確かめる判定は、期限までの時刻のドライバーの記録がすべて届いてから行う（エンジンの `driver_needed_through` が期限を返し、見張りは、ドライバーを期限の先まで読み終えるまで、期限より後のサーバーの記録を渡さない。4.6）。見張りでは、壁時計で期限から多くて L ほど遅れて判定されるが、判定の中身と `at` は再生と同じ。
- **予告を除く理由と、確かめていないこと**（U-11）: ログで確かめたのは、予告の行と切断の行の時刻が対応することまで（メモの 3.5）。予告のあった切断が、利用者の操作によるものか、トラッカーが自分で切ったもの（電池切れ、スリープ）を含むかは確かめていない。除かないと、使い終わってトラッカーの電源を切るたびに、台ごとに「接続が切れました」が push される（6 日で予告は 44 件）ので、既定では除く。電池切れなどを見落とすことになるが、そのときはトラッカーの光が消え、SteamVR でも切れて見える。オーナーの試し（14 章の 2）で、電源を切った時刻と予告の行が合わなければ、既定を `false` に戻す（設計の追補）。
  - 予告のあった切断は、案内を出さないが、集計の `power_offs`（8 章）に数える。
  - ドライバーのファイルがないとき（4.1）は、条件 5 を確かめられない（予告がないものとして扱い、案内が出うる）。その日の集計の `source.driver.found` が `false` になる。
- **直った**: `r4.back_s`（仮 600 秒）以内に `TrackerAdded(N)` が来たら `Resolved`（配信は状態の窓だけ。7.1）。来なければ `Ended`。つなぎ直すと番号 N が変わることがある（3.2）。N に別のトラッカーがつながったときと、切れたトラッカーが別の番号でつながったときを見分けるため、切れた N のシリアル番号が分かっていれば、`Resolved` は、同じシリアル番号の `SerialKnown` が来たとき（番号は問わない）にする。シリアル番号が分からなければ、`TrackerAdded(N)` で `Resolved` にする。
- R4 の条件を満たした切断は、案内を出したかどうかにかかわらず、集計の「操作と関係のない切断」に数える。

#### R5 使用中の長い喪失（足した規則）

- **足した理由**: 使っている途中で数分戻らない喪失がある（PRD の 1 章の「長い喪失」。1 分を超える喪失は 6 日で 19 回、11/27 00:18 に約 7 分。`findings-2026-11-27.md`）。W-4 の 3 つには入らないが、利用者が気づかないまま続くと困るので足す。要件に足すかは PM に判断を仰ぐ（14 章）。
- **既定では切ってある**（設定 `rules.r5.enabled = false`）。切ってあるあいだは、検出も配信もしない（`detections.R5` は 0）。集計の `long_losses`（8 章）は、R5 とは別に、状態の変わり目から数える（`r5.wait_s` の値だけを使う）。PM が判断のための数（1 晩に何回出るか）を見るときは、`openut replay --set rules.r5.enabled=true` で流す。PM が認めたら、既定を `true` に変える（設計の追補と、実装計画の 3 章の後続の修正）。
- **条件**（`rules.r5.enabled = true` のとき）: 接続してから一度は追跡中になった（`tracked_since_connect = true`）トラッカーが、追跡中でなくなってから `r5.wait_s`（仮 60 秒）たっても追跡中に戻らなければ、`Raised`（期限は追跡中でなくなった時刻 + `r5.wait_s`。期限ちょうどに戻れば出さない。6.1）。
- **直った**: 追跡中に戻ったら `Resolved`（配信は状態の窓だけ）。**終わった**: 未接続になったら `Ended`。

### 6.3 規則にしないもの

| もの | 扱い | 理由 |
|---|---|---|
| `[N] OET OVERSPEED!` | 集計に数えるだけ | 何をすればよいかの根拠がない。走り出し（R-6）の検知は、姿勢を直接読める M3 で行う |
| 短い喪失（`r5.wait_s` より短い） | 集計に数えるだけ | 6 日で 234 回あり、案内にすると多すぎる |
| `Query wifi from tracker N` が出たのに `Got host wifi info.` が来ない | 数えるだけ（`Detection` にはしない） | 実例を見ていない（メモでは数えていない）。続けて断られる問い合わせがあれば R1 が出るので、別の規則は要らない見込み。再生テストで実例が見つかれば、規則にするかを決める |
| ログが止まった（伸びない） | 見張り役の具合として持ち、利用者には、待機中と灰色（いまの値として見せない）と、すること 1 行で見せる（7.4 の「仕組みの側の不具合」） | トラッカーの不具合ではなく、見張り役が見えていないこと |

## 7. 案内の出し方（W-5）

### 7.1 配信の決まりと文言

配信の決まり（`openut_core::notice`。入出力なし）は、`Detection` を受けて `Notice` を作る。`push = true` の案内だけが、SteamVR の中とデスクトップに出る。すべての案内は、状態の窓の案内の欄に、エピソードが終わるまで出る。

| 規則 | `Raised` | `Escalated` | `Resolved` | `Ended` | 同じ案内を繰り返さない決まり |
|---|---|---|---|---|---|
| R1 | push | push（1 回だけ） | push | 欄から消すだけ | エピソードごとに 1 回。間隔は空けない（11/27 は 3 分おきに 3 回起きており、毎回知るべきため） |
| R2 | push | — | 欄から消すだけ | 欄から消すだけ | 同じトラッカーの R2 のエピソードが終わってから 5 分（`notice.cooldown_s`）以内に始まったものは push しない |
| R3 | push | — | 欄から消すだけ | 欄から消すだけ | R2 と同じ |
| R4 | push | — | 欄に「{T} がつながった」を 30 秒出してから消す | 欄から消すだけ | 同じトラッカーの R4 を push してから 5 分以内は push しない（欄の案内に回数を足す） |
| R5 | push | — | 欄から消すだけ | 欄から消すだけ | R2 と同じ。**既定では R5 を切ってあるので、何も出ない**（6.2） |

- 「同じトラッカー」は、2 つのエピソードの `serial`（3.6）が**どちらも**分かっていれば同じシリアル番号の台、どちらかが分からなければ同じ番号 N（初版は番号 N だけで見ていた。つなぎ直しで番号が変わっても、同じ台を数えるため。片方だけ分かるのは、前のエピソードがシリアル番号の来る前だったとき）。
- 「欄」は、状態の窓の異常の案内の欄（7.4 の窓の 2）。欄の文言は、表示の写しが、写しを作るたびに、その台のいまの呼び名で作る（3.2、3.6 の `NoticeText`）。push の文言は、出したときの呼び名のまま。

全体の決まり:

- **間隔**: push は、前の push から 10 秒（`notice.min_gap_s`）以上空ける。そのあいだに来た push は待たせ、まとめて 1 つにする（優先度の最も高いものを出し、見出しに「ほか N 件」を足す）。優先度は R1 > R4 > R3 > R2 > R5。
- **追いつきのあいだ**（4.5）: push しない。終わった時点で続いているエピソードを、優先度の順に、上の間隔の決まりで出す（終わった時点で、下の「VIVE Hub が止まっているあいだ」なら出さない。再構築の追いつきの終わりも同じ。止まっている・起動の途中は、再構築をまたいで続く〔5.6〕ので、作り直しで読み直した止まる前のエピソードも出さない）。
- **再構築のあと**（4.1）: 配信の決まりは、再構築の前に push 済みで続いていたエピソードを、規則とトラッカー（上の「同じトラッカー」と同じ比べ方。`conn` は再構築で振り直されるので使わない。3.6）の組で覚えている。再構築の追いつきが終わった時点で続いているエピソードのうち、規則とトラッカーが同じものは、同じエピソードの続きとみなして push し直さない（push 済みの段階も引き継ぐ。R1 で `Escalated` を push 済みなら、もう出さない）。覚えている組のうち、再構築のあと続いていないものは、欄から消すだけ（R1 の「直った」も出さない。直ったのを見たのではないため）。エピソードの番号は再構築で振り直されるので、組の照合には使わない。
- アプリを起動し直したときは、配信の決まりの覚えていることもなくなるので、続いているエピソードはもう一度 push される（起動し直した利用者に、いまの問題を知らせ直すのはよいこととする）。
- **直ったら待たせている案内を捨てる**: 待たせている push のエピソードが、出す前に `Resolved` か `Ended` になったら、出さない。
- **VIVE Hub が止まっているあいだ**（`Engine::set_hub_down(true)`。5.6 の「止まっている・起動の途中」。PR #37 のレビュー〔2 回目〕の指摘で足した）: push しない。`Notice` は作るが `push = false` にする（欄の中身〔7.4 の `live`〕には入るが、表示の写しは止まったときの見せ方で、欄を出さない）。止まる前の様子についての案内で、利用者には G1（VIVE Hub を起動する）を見せているため。
  - **止まっているあいだに期限が来た案内**も、push しない。エンジンの時刻は止まっているあいだも壁時計で進む（4.6）ので、止まる前に始まった待ちの R2・R3・R5 の `Raised`、R1 の `Escalated`、R4 の判定は、止まっているあいだにも出る（`Detection` は再生と同じ。5.6）。
  - `set_hub_down(true)` を受けたら、**待たせている push**（上の「間隔」）を捨て、**まだ `drain` で渡していない push**（その回に作った `push = true` の `Notice`）を `push = false` にする（取り消す。止まる前の様子についてのため。PR #37 のレビュー〔3 回目〕の指摘で、取り消しを足した）。`drain` が出口への区切りなので、出口に渡る push は、その回の写し（7.4）が止まった・起動の途中の見せ方でない回のものだけになり、画面と通知が食い違わない。push は、読んだ回の区切りに左右される（終了の行より前の行を前の回に読んでいれば、その行で作った push は前の回に出ている）が、`Detection` は左右されない。
  - **取り消した push**と捨てた push は、出さなかったものとして扱う: 間隔の決まり（上）の前の push にせず、同じ案内を繰り返さない決まりの前の push にせず、集計の `pushes` に数えない。そのエピソードは、止まっているあいだに `Raised` になったものと同じに扱う（次の項目）。
  - **終了の行を読んだ回から**（PR #37 のレビュー〔3 回目〕の指摘）: アプリは、`ServerStopped` を渡した直後、次の出来事を渡す前に `set_hub_down` を渡し直す（3.7、M1-9）。ログが `Stopped` になると、出口の見張りの答えが `Starting`（プロセスが見つかっている）か、ログだけで止まっている（`Unknown`）になり、`hub_down` が `true` になるので、次の確かめを待たずに、その行から push しない。そのあとの終了の途中の行（`Release begin` のあとの行）で時刻が進み、待たせていた push（たとえば R4）の間隔が過ぎても出さない。
  - 止まっているあいだに `Raised` になったエピソードと、push を取り消した・捨てたエピソードは、同じ案内を繰り返さない決まり（表の右の列）の、前のエピソード・前の push として数えない（利用者に出していないため。起動し直したあとの同じ台の案内を、そのために止めない）。
  - `set_hub_down(false)` を受けても、続いているエピソードを push し直さない（追いつきの終わりと違う）。止まる前から続くものは、止まる前に push 済みか、push を取り消した・捨てたもの（止まる前の様子についてのため）で、止まっているあいだに始まったものも、止まる前の様子についてのため。そのあとに始まったエピソードは、ふつうに push する。起動し直すと、開始の行（`ServerStarted`）で全台が未接続になり、R1・R2・R3・R5 のエピソードは終わる（6.2）。止まる前か、止まっているあいだに始まった R4 のエピソード（切れた台が戻らない）は、開始の行のあと、`r4.back_s` のあいだ欄に残る（台がつながれば `Resolved`。終了の行があって止まったときと同じ）。
  - 変わるのは `push` だけで、`Detection`、状態、集計は変えない（見張りと再生で同じ。D-22）。集計の `pushes`（8 章）には、出さなかった案内（取り消したものを含む）を数えない。
- 配信の値（`notice.cooldown_s`、`notice.min_gap_s`）も仮の値。再生テストで、1 晩に出る push の数を見て決める（6.1）。

文言（意味。見た目と言い回しの調整は UI/UX デザイン担当。意味を変えるときは設計の変更）。`{T}` は対象のトラッカーの呼び名（3.2。`serial` の台。シリアル番号が分からなければ番号のない呼び名）、`{H}` はホストの呼び名（3.2。決め方は 6.2 の R1 の「案内するトラッカー」。決まらなければ「ホストのトラッカー」）。オーナーの方針（`roles/ux.md`）に合わせ、見出し（いまの状態）と、いま何をすればよいかの 1 行だけにする。

| 規則と段階 | 見出し（意味） | すること（1 行。意味） |
|---|---|---|
| R1 `Raised` | トラッカーの追跡が始まらない | {H} の電源だけを入れ直す。ほかのトラッカーとドングルはそのまま |
| R1 `Escalated` | まだ追跡が始まらない | {H} の電源だけを入れ直す。ほかのトラッカーとドングルはそのまま |
| R1 `Resolved` | 直った | （なし） |
| R2 `Raised` | {T} の準備に時間がかかっている | {T} を {H} の近くに置いて待つ |
| R3 `Raised` | {T} の追跡が始まらない | {T} を手に持ち、部屋の中をゆっくり見せる |
| R4 `Raised` | {T} の接続が切れた | {T} の電源が入っているか確かめる |
| R4 `Resolved` | {T} がつながった | （なし） |
| R5 `Raised` | {T} の追跡が 1 分以上止まっている | {T} のカメラの前をあけ、明るい方へ向ける |

- **#27 で直した文言**（W-10）: 初版の R2「地図を受け取れていません」、R3「位置合わせが終わりません」、R5「位置を失っています」は、内部の仕組みの言葉なので、利用者から見えること（準備が終わらない、追跡が始まらない・止まっている）に替えた。R1 の「トラッカーが始まらない状態です」は、何が始まらないのかが分からない（`roles/ux.md` の「不自然な言い回しの例」）ので、「追跡が始まらない」にした。すること（1 行）は変えていない。
- **使わない言葉**（W-10。文言の表、7.4 の 1 行、手引き、トレイの 1 行のすべて）: 地図、同期、Wi-Fi、位置合わせ、位置取り直し、見張り、監視、ログ、スロット、エピソード、規則の名前（R1〜R5）、内部の状態の名前（3.5）。使ってよい言葉は、VIVE Hub の画面と、機器と、利用者の操作の言葉: トラッカー、ドングル、ホスト、電源、追跡、接続、電池、充電、VIVE Hub、SteamVR、OpenUT。M1-7 のテストで、文言の関数が返すすべての文字列に、使わない言葉が入っていないことを確かめる。
- 文言の既定（`openut_core::text` が返す文字列）は、この表の意味を丁寧な形にしたものを、M1-7 で仮に置く（例: 「トラッカーの追跡が始まりません」「{H}の電源だけを入れ直してください。ほかのトラッカーとドングルは、そのままにします」）。UI/UX デザイン担当の文言（`docs/design/ui/`。yomiyasu で推敲したもの）が main に入ったら、そちらに替える（意味が同じなら設計の変更は要らない）。
- R1 の文言は W-3 のとおり、全台の電源オフとドングルのリセットを勧めない。「ほかのトラッカーとドングルはそのまま」をはっきり書くのは、11/27 にドングルのリセットと再ペアリングを 3 回やって 3 回とも再発しているため（`findings-2026-11-27.md`）。
- R3 の「部屋の中を見せる」は、静止したまま起動すると位置合わせに失敗し続けた記録（`findings-2026-11-28.md`）と、PRD の R-8 による。
- R5 の「カメラの前をあけ、明るい方へ」は、HTC の公式の案内（追跡が安定しないとき: 障害物から離す、本が読める明るさ。`findings-2026-11-27.md` の「HTC の公式情報」）による。
- `r5.wait_s` を変えたら、R5 の見出しの「1 分」も、その値に合わせる（文言の関数に秒数を渡す）。

### 7.2 SteamVR の中

- **方式**: OpenUT が自分のオーバーレイ（OpenVR の `IVROverlay`）を 1 枚持ち、案内を出すときだけ、HMD に対して固定した位置に表示する。文字は OpenUT が画像（RGBA）に描いて `SetOverlayRaw` で渡す。
  - SteamVR の通知の仕組み（`IVRNotifications_002` の `CreateNotification`）は使わない。現行のヘッダー（ValveSoftware/openvr の `headers/openvr.h`）にあり、オーバーレイのハンドルが要ると書かれているが、現行の SteamVR で表示されるか、どう見えるかを確かめていないため。M1-12 の試作で表示されると分かれば、使うかを決め直す。
- **SteamVR とのつなぎ方**: 種類 `VRApplication_Background` で初期化する（ヘッダーの説明では「SteamVR が動いていなければ起動せず、ほかがすべて終わったら SteamVR を動かし続けない」）。つながらなければ 10 秒ごとに試し直す。つながっているあいだは、SteamVR のイベントを 500 ms ごとに読み、終了のイベントが来たら OpenVR を閉じて、試し直しに戻る。`Background` の種類でオーバーレイを作れるかは未確認（M1-12 の試作で確かめる。作れなければ、SteamVR が動いていることを確かめてから `VRApplication_Overlay` で初期化する）。
- **出し方**: push の案内を受けたら、画像を描き直して表示し、10 秒（`steamvr.show_s`。仮）で隠す。表示中に次の push が来たら、描き直して 10 秒を数え直す。位置・大きさ・色は UI/UX デザイン担当が決める（7.5）。
- **クレート**: `openvr` 0.9.0（MIT）。`openvr_sys` 2.1.4（MIT）を通して OpenVR（BSD 3 条項の形。ValveSoftware/openvr の `LICENSE` で確認）を静的にリンクする。オーバーレイの作成、画像、HMD に対する配置、表示の API がある（クレートの `src/overlay.rs` で確認）。初期化（`openvr::init`）は `unsafe fn` なので、この呼び出しの場所だけ `unsafe` を許す（理由を PR の説明に書く。`roles/impl.md`）。
  - `openvr_sys` は、ビルドのときに CMake と C++ のコンパイラーで OpenVR をビルドし、bindgen（libclang が要る）で型を作る（クレートの `build.rs` で確認）。Windows では MSVC が要る（README で確認）。**CI の Windows で通るかは未確認**。通らなければ、代案として、`libloading` 0.9.1（ISC）で実行時に `openvr_api.dll` を読み、OpenVR の C の関数表（`VR_GetGenericInterface` の `FnTable:` の形）を自前の最小の宣言で呼ぶ（`unsafe` の範囲は、その 1 つのモジュール）。どちらにするかは M1-12 の最初に決め、PR の説明に書く。
  - Windows だけの依存にする（`[target.'cfg(windows)'.dependencies]`）。Linux の CI ではビルドしない。
- **文字の描画**: 書体は、実行時に Windows の書体のフォルダーから読む（レポジトリと配布物に書体を入れない）。候補は Yu Gothic UI、メイリオ、MS ゴシック の順。日本語以外の Windows にこれらがあるかは未確認。どれもなければ、SteamVR の中の表示をやめ、デスクトップと状態の窓だけにする（理由はアプリのログにだけ書く。仕組みの側の言葉を画面に出さないため。W-10）。描画のクレートは `fontdue` 0.9.5（MIT OR Apache-2.0 OR Zlib）か `ab_glyph` 0.2.32（Apache-2.0）。状態の窓（egui）と同じものにできるなら、そろえる（M1-12 で決める）。
- 毎フレームの描画はしない。画像を渡すのは案内を出すときだけ（12 章）。

### 7.3 デスクトップ

- **方式**: Windows のトースト通知。クレートは `tauri-winrt-notification` 0.8.1（MIT OR Apache-2.0。tauri-apps。2026-07 に更新）。
- 見出しと 1 行を、そのまま 2 行にして出す。音は既定のまま。トーストを押したら状態の窓を開く（押したことを受け取れるかは未確認。受け取れなければ、トレイから開く）。
- トーストには、送り主のアプリの ID（AppUserModelID）が要る。クレートの説明では、インストールしていないプログラムは、当面 PowerShell の ID（`Toast::POWERSHELL_APP_ID`）を使えるとある。その場合、送り主が「Windows PowerShell」と表示される。OpenUT 自身の ID を登録する方法（スタートメニューのショートカットか、レジストリ）は、Microsoft の公式の文書をこの環境から読めなかったので未確認。M1-10 で確かめ、確かめられるまでは PowerShell の ID を使う。
- SteamVR が動いているときも出す（W-5 は両方に出すことを求めている。設定でどちらも切れる）。

### 7.4 画面: 何をいつ見せるか（W-10〜W-12）

画面は 3 つ: トレイのアイコン（常駐）、状態の窓（開いたときだけ）、通知（7.2、7.3）。画面なしで動かす `openut watch`（コンソールに push の案内を出す）もある（M1-9）。#27 で、オーナーの指示（D-35。PRD の W-10〜W-12）に合わせて、この節を書き直した。

**見せるもの**（W-10）: 次の 3 つと、トラッカーとドングルの項目（W-11）だけ。

1. 装着と準備の手引き（下の「手引き」）。
2. いまの状態が、正常・待機中・異常のどれか（全体と、トラッカーごと。下の「利用者に見せる状態」と「全体」）。
3. 異常のときに、利用者がすること（7.1 の「すること」と、下の「仕組みの側の不具合」の 1 行）。

**見せないもの**: 内部の状態の名前（3.5。接続、Wi-Fi 待ち、地図同期中、位置取り直し中、追跡中）、PC の Wi-Fi 情報、見張り役の具合（うまく動いているときの「見張り中」「監視中」の印も）、ログのファイルの名前と日付、読めなかった行の数、スロットの番号 N、その状態になってからの時間、日ごとの集計、規則とエピソードの名前。これらは、アプリのログと集計のファイル（8 章、9.3）にだけ書く。初版の状態の窓の 1（見張り役の状態の行。「見張り中（11/28 のログ）」）、3 の内部の状態の名前と経過時間、4（PC の Wi-Fi 情報）、5（今日の集計）と、F3（名前を付ける）は、やめた。

**表示の写し**（`openut_core::display`。入出力なし。#27 で足した）: 画面に出す中身は、次の入力から 1 つの関数で作る。トレイ、状態の窓、`openut watch` は、これを描くだけにする（M3 でも同じものを使う。R-9）。

```rust
/// How well the app can watch (computed by the app, M1-9). Never shown as words (below).
pub enum WatchHealth { Ok, NotFound, Unreadable, Stalled, FormatMismatch }
/// 正常、待機中、異常: the three states of W-10, for the whole and per tracker. No fourth state:
/// when the values cannot be confirmed, the whole is Waiting and `greyed` is true (below).
pub enum Overall { Ok, Waiting, Fault }
pub enum UserState { Ok, Waiting, Fault }
pub enum GuideStepId { G1, G2, G3, G4, G5 }
pub struct GuideStep { pub id: GuideStepId, pub done: bool, pub progress: Option<(u16, u16)> }
pub enum Line { Fault { headline: String, action: String }, Guide(GuideStepId), Health(WatchHealth) }
/// One live notice in the window's list, in words made with the labels of this model (below).
pub struct FaultRow { pub headline: String, pub action: String, pub resolved: bool }   // R4 "つながった": resolved
/// hub: the answer of HubGate (5.6). It replaced `hub_process: Option<bool>` (PR #37, review 2).
pub fn view(snapshot: &Snapshot, live: &[Notice], health: WatchHealth,
            hub: HubLive, catching_up: bool) -> DisplayModel;
pub struct DisplayModel {
    pub overall: Overall,
    pub line: Option<Line>,          // the one thing to do now (below)
    pub faults: Vec<FaultRow>,       // the live notices, newest first (the window's list)
    pub guide: Vec<GuideStep>,       // G1..G5 (below)
    pub dongle: DongleState,
    pub devices: Vec<DeviceRow>,     // in the order of the list (below)
    pub greyed: bool,                // the values cannot be confirmed now: none is shown as current
    pub pick_folder: bool,           // show "フォルダーを選ぶ" (F4): only when health is NotFound
}
pub struct DeviceRow {
    pub label: TrackerLabel, pub serial: Option<Serial>, pub connection: Connection,
    pub battery: Option<Battery>, pub state: Option<UserState>, pub firmware: Option<String>,
    pub host: bool,
}
```

- `live` は、配信の決まり（7.1）が持つ「欄」の中身: 続いているエピソード（`Raised` のあと `Resolved` も `Ended` も出ていないもの）の最新の `Notice` と、R4 の「つながった」を出している 30 秒のあいだの `Resolved`。
- `line` の文言は、`Fault` では案内の文言（7.1）、`Guide` と `Health` では `openut_core::text` が作る（手引きの段、下の「仕組みの側の不具合」の 1 行）。
- **欄の文言は、この写しの呼び名で作る**（PR #37 のレビューの推奨）: `faults` と、`line` の `Fault` の文言は、`Notice::text`（3.6）の穴に、その案内が当たる台（下の「案内の当て方」）の `DeviceRow::label` を入れて作る。当たる台がなければ、`Notice::target`（出したときの呼び名）を入れる。そのため、枠の呼び名と欄の呼び名は、いつも同じ写しの中でそろう（末尾の長さが変わったときも、VIVE Hub の番号が振り直されたときも、仮の台がシリアル番号の分かった台に替わったときも。3.2）。push の文言は変えない。
- `DisplayModel` と `DeviceRow` には、内部の状態（`Phase`）、PC の Wi-Fi 情報、スロットの番号（`TrackerKey`）、接続の番号（`ConnId`）、経過時間、集計の値を入れる欄を作らない（見せないものが画面に紛れ込まないよう、型で守る。`DeviceView` の `key`、`conn`、`phase` は、写しを作るときに使うだけ）。

**利用者に見せる状態**（トラッカーごと。`DeviceRow::state`。上から順に、最初に当てはまるもの）

| 見せる状態 | 条件 |
|---|---|
| 異常 | その台に当たる、続いている案内がある（`live` の `Raised`・`Escalated`。下の「案内の当て方」）。未接続の台でも（R4 の切れた台、R1 の接続の終わったホスト） |
| なし（`None`） | 未接続（接続の状態の「未接続」で見せる） |
| 正常 | 内部の状態が追跡中 |
| 待機中 | 未接続でなく、追跡中でない（内部の状態が接続、Wi-Fi 待ち、地図同期中、位置取り直し中） |

- **案内の当て方**（PR #37 のレビューの指摘で直した）: 案内の `serial`（3.6）があれば、同じシリアル番号の台。`serial` がなければ、案内の `conn`（3.6）が、いまの接続（`DeviceView::conn`）と同じ台。どちらもなければ、どの枠にも当てない。
  - `conn` でも当てるのは、案内を出したときにシリアル番号がまだ来ていなかった台（仮の台。5.4）が、同じ接続のあいだに `SerialKnown` で台帳の台に替わっても、異常のまま見せるため。**例**: 途中から読み始め、番号 2 の仮の台（接続 c）がホスト（報告。5.3）で、R1 が出る（`serial` なし、`conn` = c）→ 仮の台が異常。同じ接続で `SerialKnown(2, S)` が来る → 仮の台は消え、S の枠（いまの接続が c）が異常になる。欄の案内の呼び名も、同じ写しで「ホストのトラッカー」から S の呼び名に替わる（上）。
  - **つなぎ直して同じスロットを別の台が使ったとき**は、接続が別（`conn` が違う）なので当てない。**例**: 上の例の続きで、S が番号 2 で切れ、別の台 B が番号 2 でつながる（接続 d）。R1 が続いていても、案内の `conn` は c なので、B の枠は異常にならない（全体と欄だけが異常）。R1 が `Escalated` で案内する台を決め直したら（3.6）、そのときの `serial` と `conn` で当てる（ホストの根拠に S のシリアル番号があれば、S の枠。6.2 の R1 の 2）。
  - R4 の `conn` は切れた接続で、いまその接続の台はない。そのため、シリアル番号の分からない台の R4 は、どの枠も異常にせず、全体と欄だけで見せる（切れたあと、同じスロットに別の台がつながりうるので、スロットでは当てない）。
- R1 では、案内する台（ホスト。6.2）が異常になり、Wi-Fi 情報を待っているクライアントは待機中のまま（利用者が手を動かす台は、ホストだけのため）。案内する台が決まらない R1（`serial` も `conn` も `None`）では、どの台も異常にならず、全体だけが異常になる。
- 案内の出ない待ち（R2・R3 のしきい値の前。R5 を切ってある既定では、追跡中でなくなった台も）は、待機中のまま（6.2）。
- R4 の `Resolved`（つながった）では、異常にしない。

**全体**（トレイと、窓の上の段。`DisplayModel::overall`、`line`、`greyed`。上から順に、最初に当てはまるもの）。全体も、W-10 の 3 つ（正常・待機中・異常）だけで見せる（PR #37 のレビューの指摘。初版の 4 つ目「確かめられない」はやめ、待機中の灰色にした）。

| 順 | 全体 | 条件 | `line`（いますること） | ほか |
|---|---|---|---|---|
| 1 | 待機中 | VIVE Hub が止まっている: 出口の見張りが `Stopped`（プロセスがない。5.6）。追いつきのあいだも、見張り役の具合が何でも | `Guide(G1)`（VIVE Hub を起動する） | 下の「止まったときの見せ方」 |
| 2 | 待機中 | 見張り役の具合が `NotFound` | `Guide(G1)` | 台は 0 台（ログを読んでいない） |
| 3 | 待機中 | 追いつき（4.5）のあいだ。ただし、出口の見張りが `Starting` なら 5（下の注） | なし | 灰色（`greyed`） |
| 4 | 待機中 | ログで VIVE Hub が止まっている（`HubState` が `Stopped`）で、出口の見張りが `Unknown`（プロセスを確かめていない。5.6） | `Guide(G1)` | 下の「止まったときの見せ方」 |
| 5 | 待機中 | **起動の途中**: 出口の見張りが `Starting`（プロセスは見つかったが、止まったと確かめたあとの開始の行を、まだエンジンに渡していない。5.6）。追いつき（再構築の追いつきを含む）のあいだも。`Running` でログが `Stopped` の組も同じに扱う（出口の見張りは、この組を `Starting` にする。写しは、どの組を受けても同じ答えにする） | なし。見張り役の具合が `Ok` でなければ `Health`（開始の行が来ないときの `FormatMismatch` など。5.6） | 下の「止まったときの見せ方」（G1 は済み）。灰色にしない |
| 6 | 待機中 | 見張り役の具合が `Unreadable`、`Stalled`、`FormatMismatch` | `Health`（下の「仕組みの側の不具合」の 1 行） | 灰色（`greyed`） |
| 7 | 異常 | `live` に `Raised`・`Escalated` の案内がある | 最も優先度の高い案内（7.1 の優先度。同じなら新しいもの）。そのすること | |
| 8 | 正常 | 未接続でない台が 1 台以上あり、そのすべてが正常 | `Guide(G5)`（身につける） | |
| 9 | 待機中 | そのほか | 手引きの、いまの段（下） | |

- 1・4・5 は、5.6 の「止まっている・起動の途中」（`hub_down`）にあたる。このとき、通知の出口も push しない（7.1 の「VIVE Hub が止まっているあいだ」）。画面と通知で、同じ `HubLive` を使う。
- **`pick_folder`**（「フォルダーを選ぶ」を出すか。PR #37 のレビューの指摘で足した）は、順とは別に、見張り役の具合が `NotFound` のときだけ `true`。1・2・4 は、どれも待機中と `Guide(G1)` になるので、ログが見つからないのか、VIVE Hub が止まっているのかを、画面はこの値で分ける。1 と `NotFound` が重なれば、G1 とボタンの両方を出す。
- **1 を、追いつきと見張り役の具合より先にする**（PR #37 のレビューの指摘）: プロセスの確かめは、ログとは別に、いま VIVE Hub が動いていないことを示す。利用者がすることは VIVE Hub を起動することなので、「VIVE Hub を起動し直す」より G1 が合う（`Stopped` のあいだは `Stalled` にもしない。5.6）。終了の行のないまま止まったときも、`Stalled` になる 10 分を待たずに、次のプロセスの確かめ（10 秒ごと。5.6）で G1 を出せる。
- **5 を、見張り役の具合より先にする**（PR #37 のレビュー〔2 回目〕の指摘）: 起動し直した VIVE Hub の開始の行を渡すまでは、ログから組み立てた値は止まる前のもので、灰色（最後に読めた値）でも見せない。ふつうは数秒で終わる（5.6 の「開始の行が来ないとき」）ので、追いつきと同じく `line` を出さない。開始の行が `watch.start_wait_s` を超えて来なければ、見張り役の具合が `FormatMismatch` になり、その 1 行を出す（台の値は、止まったときの見せ方のまま）。
- **5 を、追いつきの 3 より先にする**（PR #37 のレビュー〔3 回目〕の指摘）: 起動の途中に作り直しで再構築すると（4.1。ドライバーのファイルだけのときも）、再構築の追いつきで読み直すのは止まる前のセッションで、その値は、灰色（最後に読めた値）でも、いまの値として見せない。追いつきが終わっても、新しい開始の行を渡すまでは 5 のまま（出口の見張りは、再構築で開始待ちを下ろさない。5.6）。起動したときの追いつきで、前のセッションの終了の行を読んでから次の開始の行を読むまでも、出口の見張りが `Starting` になるので 5 になるが、終了の行のあとの状態の写しは全台が未接続（5.1）なので、3 の灰色の見せ方と、台の並びのほかは変わらない。
- **灰色**（`greyed`。3 と 6）: 正常・待機中・異常のどれかを、ログから確かめられないとき。全体は待機中にし（正常と見せて安心させたり、異常と見せて手を動かさせたりしないため）、トラッカーの項目、ドングルの値、欄の案内は、最後に読めた値として灰色で見せる（いまの値として見せない。見た目は UI/UX デザイン担当）。いますることは、6 では「仕組みの側の不具合」の 1 行、3 ではなし（追いつきは、ふつう数秒で終わる。4.5、12 章）。灰色は 4 つ目の状態ではなく、待機中の見せ方の 1 つ（W-10 の 3 つのまま。4 つ目を足すかは 14 章の 6）。

**止まったときの見せ方**（全体の 1・4・5。5.6 の「止まっている・起動の途中」。PR #37 のレビューの指摘〔1 回目・2 回目〕で足した）: ログから組み立てた値は止まる前のもの（終了の行のないまま止まれば、接続、ホスト、電池、続いている案内が残っている。5.6）なので、いまの値として見せない。起動し直して、プロセスが見つかっても（5）、止まったと確かめたあとの開始の行（止まった印より後の時刻のもの。5.6）を渡すまでは、同じに見せる（作り直しの再構築をまたいでも）。

| 項目 | 見せ方 |
|---|---|
| 全体、`line` | 待機中。`line` は、止まっている（1、4）なら `Guide(G1)`、起動の途中（5）ならなし（見張り役の具合が `Ok` でなければ `Health`）。灰色にしない（1・4 は、すること〔VIVE Hub を起動する〕が分かっていて、5 は、値を灰色でも見せないため） |
| トラッカーの項目 | 台帳の台を、すべて未接続として並べる（`connection = Disconnected`、電池なし、状態なし、ホストの印なし、呼び名は末尾だけ〔3.2〕。シリアル番号と版は台帳の値）。仮の台は並べない |
| 欄（`faults`）と、台の異常 | 出さない（どの台も異常にしない）。止まる前の状態についての案内のため（通知の出口も push しない。7.1） |
| ドングル | `Unknown`（分からない。5.5 の `ServerStopped` と同じ） |
| 手引き | 止まっている（1、4）: いまの段は G1。G2〜G5 は済んでいないとして見せる（下の「いまの段」）。起動の途中（5）: G1 は済み。ほかの段は、この表の値（ドングルは分からない、台は未接続）で決める（いまの段は G3） |

- 状態の写しと規則（エンジン）は変えない（プロセスの確かめは、規則と集計に使わない。5.6、D-22）。VIVE Hub を起動し直すと、開始の行（`ServerStarted`）で、状態の写しも全台が未接続から始まる（5.1）。出口の見張りは、その開始の行を渡した直後に `Running` になり（5.6）、写しはふつうの見せ方に戻る。
- 終了の行があって止まったとき（4。プロセスを確かめていて、見つかっていれば 5）は、5.1 の `ServerStopped` で、状態の写しはすでに全台が未接続で、ドングルも `Unknown`。残りうるのは、続いている R4 の案内（`r4.back_s` のあいだ）だけで、それも出さない。
- **止まった直後と `Stalled` が重なったとき**: どちらでも 1 が先に当たるので、見え方は同じ（G1、全台が未接続、欄なし）。`Stopped` になってからは、`Stalled` にもしない（5.6）。プロセスの確かめのない環境（`Unknown`。Linux と再生）で、終了の行のないまま止まったときは、4 が当たらず、10 分後に 6（`Stalled`。灰色と「VIVE Hub を起動し直す」）になる。
- **開始の行が遅れて届く再起動**（5）: 例: 3 台が追跡中で、ホストに R1 が続いているまま、終了の行なしで止まる → 次の確かめで 1（G1、全台が未接続、欄なし。push もしない）。利用者が VIVE Hub を起動する → 次の確かめでプロセスが見つかり、5（待機中、`line` なし、全台が未接続、欄なし。push しない）。止まる前の追跡中の 3 台と R1 の案内は、戻さない → 数秒後に開始の行を読んでエンジンに渡す → `Running`（全台が未接続から。ドングル、トラッカーがつながると、ふつうに見せる。push もふつうに出す）。
- **起動の途中に、ファイルが作り直されたとき**（5。PR #37 のレビュー〔3 回目〕の指摘）: 上の例で、プロセスが見つかって 5 のあいだに、ドライバーのファイルが置き換えられる → 両方を読み直す（4.1 の再構築）。再構築の追いつきのあいだも、終わってからも 5（読み直した止まる前の追跡中の 3 台と R1 の案内、止まっているあいだに出た R2 を、灰色でも見せない。追いつきの終わりにも push しない）→ 新しい開始の行を読んで渡す（止まった印より後の時刻）→ `Running`。作り直したサーバーのファイルに、新しい開始の行がすでにあれば、再構築の追いつきの中で `Running` になり、追いつきの終わりに、新しいセッションの値と、続いている案内をふつうに出す。
- **終了の行を読んだ回**（5。PR #37 のレビュー〔3 回目〕の指摘）: プロセスが見つかっているまま、1 回の読み取りで `Release begin` と終了の途中の行を読む → その回の写しは 5（ログが `Stopped`）で、その回から push しない（その回に作った push も、待たせていた push も取り消す。7.1）。

**手引き**（W-10 の (a)。`DisplayModel::guide`）: 使い始めるときの段を、この順に見せる。どの段が済んだかは、ログ（と、VIVE Hub のプロセスの確かめ。5.6）から決める。言い回しは UI/UX デザイン担当。

| 段 | 利用者がすること（意味） | 済んだとする条件 |
|---|---|---|
| G1 | VIVE Hub を起動する | 見張り役の具合が `NotFound` でなく、VIVE Hub が止まっていない（全体の 1 と 4 でない。起動の途中〔5〕は済んだとする。5.6） |
| G2 | ドングルを PC につなぐ | ドングルが `Connected` か `Unknown`（5.5。分からないときは、利用者に手間を頼まない） |
| G3 | トラッカーの電源を入れる | 未接続でない台が 1 台以上 |
| G4 | 追跡が始まるのを待つ。始まらなければ、トラッカーを部屋へ向けて、ゆっくり動かす（カメラの前をふさがない） | 未接続でない台のすべてが正常。済んでいないあいだは、`progress` に、正常の台数と、未接続でない台数を入れる（例: 2/3） |
| G5 | トラッカーを身につける | なし（ログからは分からない。G1〜G4 が済んだら、いまの段として見せる） |

- **いまの段**は、最初の済んでいない段（G1〜G4 がすべて済んでいれば G5）。いまの段より後の段は、条件を満たしていても「済んだ」と見せない（`done = false`。例: VIVE Hub が止まっているときに、G2〜G4 を済んだと見せない）。
- 異常があるときも、手引きは見せる（いますることの 1 行は、異常の案内を先にする。上の「全体」）。
- G4 の「部屋へ向けて動かす」は、静止したまま起動すると数分のあいだ位置合わせに失敗し続けた記録（`findings-2026-11-28.md`）による（R3 の案内と同じ根拠。7.1）。G4 を G5（身につける）より前にしたのは、カメラが部屋を見渡せる向きで追跡を始めるため（設計の案。順を変えるかは、UI/UX デザインの確認で、オーナーに見てもらう。14 章）。
- 体の部位は書かない（W-12。どこに付けるかは、利用者の使い方による）。
- 手引きは、状態の窓にだけ出す（push しない。VR の中とデスクトップには、7.1 の push の案内だけを出す）。

**仕組みの側の不具合**（W-10。見張り役の具合 `WatchHealth`。アプリ〔M1-9〕が決めて、表示の写しに渡す）

| 具合（内部） | 条件 | 全体 | 利用者に見せる 1 行（すること。意味） |
|---|---|---|---|
| `Ok` | いまのファイルを読めている | （上の「全体」のとおり） | — |
| `NotFound` | ログのフォルダーか `ViveTrackerServer_*.txt` が見つからない（4.1、9.2） | 待機中 | 手引きの G1「VIVE Hub を起動する」。窓には「フォルダーを選ぶ」（F4。`pick_folder`）も出す（VIVE Hub を既定と違う場所に入れた人のため） |
| `Unreadable` | 読み取りが 3 回続けて失敗した（共有の違反、権限など） | 待機中（灰色） | 「OpenUT を起動し直す」 |
| `Stalled` | 未接続でない台があるのに、サーバーのファイルが 10 分（仮。`summary.gap_s` と同じ値。8.1）伸びない（下の注）。出口の見張りが `Stopped`・`Starting` のあいだは、にしない（5.6） | 待機中（灰色） | 「VIVE Hub を起動し直す」 |
| `FormatMismatch` | その日の `unparsed` が 100 行（仮）を超えた。または、その日のサーバーの記録が 2,000 件（仮）を超えたのに、4.4 の表の種類が 1 件もない（4.7）。または、起動の途中（5.6 の `Starting`）が `watch.start_wait_s`（仮 60 秒）を超えた（開始の行を渡したら、この理由のものは解く） | 待機中（灰色） | 「OpenUT を新しい版にする」（VIVE Hub の版が変わり、ログの形が変わったと見るため） |

- **理由は画面に出さない**（W-10。オーナーの指示: 内部の実装に関わるエラーの文言を出さない）。どの具合になったか、その理由（読めなかった理由、伸びない時間、読めなかった行の数と例）、ログのフォルダーの場所は、アプリのログにだけ書く。利用者に見せるのは、全体の見え方と、すること 1 行だけ。初版の「ログが見つからない」「ログを読めない（理由を出す）」「ログが止まっている」「ログの形が想定と違う行が N 行」の表示は、やめた。
- **どれも push しない**（VR の中とデスクトップには出さない）。トラッカーの不具合ではなく、すぐに手を動かすことでもないため。トレイの見え方と、窓の上の段で分かる。
- VIVE Hub が止まったとき（5.6）は、見張り役の具合ではなく、手引きの G1 で見せる（利用者がすることが「VIVE Hub を起動する」で、仕組みの側の不具合ではないため）。具合が `Stalled` などでも、止まっていると分かれば G1 を先にし、トラッカーは未接続として見せる（上の「全体」の 1 と 4、「止まったときの見せ方」）。起動の途中（5）も、トラッカーは未接続として見せ、具合が `Ok` なら 1 行を出さない。
- **`Stalled` の数え方**: アプリが最後にサーバーのファイルの伸びを見た時から、壁時計で数える。**起動と再構築の追いつきの終わりには、壁時計 − サーバーで最後に読んだ記録の時刻**（どちらも同じ PC の時計）から数え始める。VIVE Hub が終了の行のないまま止まり（5.6）、前の回の状態のまま「つながっている」台が残っているときに、10 分待たずに気づくため。ずれの読み違い（U-3）でこの差が実際より大きく出ても、VIVE Hub が動いていれば、トラッカーがつながっているあいだは数秒でファイルが伸びる（記録の間隔は 99% が 8.2 秒以内。メモの 3.5）ので、`Ok` に戻る。伸びたら `Ok` に戻る。出口の見張り（5.6）が `Stopped`・`Starting` のあいだは `Stalled` にせず、`Running`・`Unknown` に戻った時から数え直す（起動し直したときは、開始の行でファイルが伸びている）。

**トラッカーの項目**（W-11。`DeviceRow`。1 台 1 つの枠）

| 項目 | 元 | 読めないとき（見せ方の意味。印は UI/UX デザイン担当） |
|---|---|---|
| 呼び名（VIVE Hub の番号と、シリアル番号の末尾） | 末尾は台帳（5.4）、番号はスロットの行のいまの接続の値（5.1）。3.2 | 3.2 の表（つながっていない台と、番号の読めない台は末尾だけ。どちらもなければ番号のない呼び名） |
| シリアル番号（全体） | 台帳 | 仮の台（5.4）は、分からない印 |
| 接続の状態 | `DeviceView::connection`（未接続、ドングル、USB） | 分からないことはない（灰色のときは、最後に読めた値を灰色で見せる。VIVE Hub が止まっているときは、未接続。上の「全体」） |
| 電池の残り（% と、充電中か） | スロットの行の `battery`（5.1。つながっているあいだだけ） | 未接続なら見せない（ログから分からない）。つながっていて `battery` がないとき（状態の行がまだ来ない、`percentage: 0`。4.4）は、分からない印（0% と見せない）。`charging` がなければ、充電中の印を付けない |
| 状態（正常・待機中・異常） | 上の「利用者に見せる状態」 | 未接続で異常でなければ、見せない |
| ファームウェアの版 | 台帳の `firmware` | 分からない印 |
| ホスト | `DeviceView::host`（ホストの接続が続いている台だけ。3.5） | 印を付けない |

- **並べ方**（W-12。PR #37 のレビューの推奨と、U-18 の答えで直した）: 台帳の台を**シリアル番号の順**（文字列の順）に並べ、仮の台は最後に、スロットの番号の順（スロットの番号は見せない）。初版の「VIVE Hub の番号の順」はやめた。番号はつなぎ直すたびに振り直される（U-18）ので、番号の順では、つなぎ直すたびに台の位置が入れ替わり、同じ番号の台のつながり方が入れ替わっても順が変わるため。
  - **並びは、ホストの交代、つなぎ直し（スロットの番号と VIVE Hub の番号の変化）、接続の状態、台数で変わらない**。台が増えたら、その台がシリアル番号の位置に入るだけで、ほかの台どうしの順は変わらない。仮の台が台帳の台に替わると、最後からシリアル番号の位置に移る（接続の直後の 1 秒ほど。3.2）。3 台でも 5 台でも、同じ形の枠を並べる（5 台で崩れない配置は UI/UX デザイン担当。#28）。
  - VIVE Hub の画面がどの順に並べているかは確かめていない（U-18。14 章のオーナーの 5 で見比べる）。
  - **例**（台の追加とつなぎ直し。シリアル番号は見本の形の置き換え）: 台帳に S1 = `00XX0000A0001`、S2 = `00XX0000A0002`、S3 = `00XX0000A0003` があり、3 台ともつながっていて、VIVE Hub の番号が 1・2・3。

    | 出来事 | 並び（上から） | 呼び名 |
    |---|---|---|
    | はじめ | S1、S2、S3 | `トラッカー 1（…0001）`、`トラッカー 2（…0002）`、`トラッカー 3（…0003）` |
    | S1 が切れる | S1、S2、S3 | S1 は `トラッカー（…0001）`（未接続。番号を付けない）。ほかは同じ |
    | S1 と S3 がつなぎ直し、VIVE Hub が S1 に 3、S3 に 1 を振る | S1、S2、S3 | `トラッカー 3（…0001）`、`トラッカー 2（…0002）`、`トラッカー 1（…0003）` |
    | ホストが S2 から S3 に替わる | S1、S2、S3 | 変わらない（ホストの印だけが移る） |
    | S4 = `00XX0000B0001` がつながる（番号 4） | S1、S2、S3、S4 | 末尾 4 文字（`0001`）が S1 と重なるので、全台を 5 文字にする: `トラッカー 3（…A0001）`、`トラッカー 2（…A0002）`、`トラッカー 1（…A0003）`、`トラッカー 4（…B0001）`。欄に S1 の案内があれば、その呼び名も同じ写しで `…A0001` になる（上の「欄の文言」） |
- 枠の数は、台帳の台（16 台まで。5.4）と仮の台の数。台帳にある未接続の台は、未接続の枠として並ぶ（その日か前の日に使った台。5.4）。
- 電池の値は、状態の行が来るたびに変わる（つながっている台ごとに数分おき。メモの 3.1）ので、最大で数分前の値。電池が少ないときの案内は作らない（W-11 は見せるだけ。案内にするかは 14 章で PM に聞く）。

**ドングルの項目**（W-11）: 5.5 の `DongleState` を、つながっている・つながっていない・分からない、の 3 つで見せる。

**トレイのアイコン**

- 3 つの見え方: 全体の 3 つ（正常、待機中、異常）。灰色（`greyed`）のときも、待機中の見え方にする（4 つ目の見え方は作らない。W-10）。
- 指を置いたときの 1 行: 全体の名前と、正常の台数／未接続でない台数（例: 「正常 3/3」「待機中 2/3」）。異常のときは、最も優先度の高い案内の見出し。`line` が `Health` のとき（灰色と、起動の途中の具合の 1 行）と、VIVE Hub が止まっているとき（`Guide(G1)`）は、全体の名前とその 1 行で、台数を出さない（いまの台数として確かめられないため）。追いつきのあいだと、起動の途中（`line` なし）は、全体の名前だけ。言い回しは UI/UX デザイン担当。
- メニュー: 「状態を開く」「終了」。左クリックでも状態の窓を開く。

**状態の窓**（横長のデスクトップの窓。D-34。閉じてもアプリは終わらない。トレイから開く。配置は UI/UX デザイン担当で、VIVE Hub の配置を参考にする。W-10）

中に置くもの:

1. 全体（正常・待機中・異常。灰色のときは、待機中の灰色の見せ方）と、いますることの 1 行（`line`）。`pick_folder` のときは「フォルダーを選ぶ」ボタン（見張り役の具合が `NotFound` のときだけ。VIVE Hub が止まっているだけのときは出さない）。
2. 異常の案内の欄: 続いている案内の見出しとすること（新しいものが上。なければ欄ごと出さない。文言は、この写しの呼び名で作る。上の「欄の文言」）。R4 の「つながった」は 30 秒（7.1）。
3. 手引き: G1〜G5 と、済んだ段の印、いまの段。全体が正常のときは、たたんでよい（UI/UX デザイン担当）。
4. ドングルの項目。
5. トラッカーの枠の並び（トラッカーの項目）。
6. 設定: 通知（VR の中、デスクトップ）を出すかどうか。

**操作の流れ**

| 流れ | 手順 |
|---|---|
| F1 起動 | 起動 → ログのフォルダーを探す（9.2）→ 見つかれば、窓を開かずにトレイに常駐し、追いつき（4.5）のあと全体の状態を見せる。見つからなければ（`NotFound`）、状態の窓を開いて、手引きの G1 と「フォルダーを選ぶ」（`pick_folder`）を出す |
| F2 案内 | push の案内 → VR の中とデスクトップに出る → 状態の窓の異常の案内の欄に残り、その台の枠が異常になる → 直ったら欄から消える（R1 は「直った」も push） |
| F3 | （やめた。初版の「トラッカーに名前を付ける」。呼び名は VIVE Hub の番号とシリアル番号の末尾で決まる。3.2） |
| F4 フォルダーを選ぶ | 「フォルダーを選ぶ」→ フォルダーの選択の画面 → 選んだフォルダーに `ViveTrackerServer_*.txt` があれば設定に保存して、追いつきのあと全体の状態を見せる。なければ、そう出して選び直してもらう。この画面の題と、選び直しの 1 行だけは、どのフォルダーかを示すために「ログ」の語を使ってよい（7.1 の使わない言葉の例外） |
| F5 終了 | トレイのメニューの「終了」→ 集計を書き出してから終わる |

**画面の作り方（Rust）**

| 部品 | クレート（2026-11-28 に crates.io で確認） | ライセンス | 保守 | 確かめたこと・未確認 |
|---|---|---|---|---|
| トレイ | `tray-icon` 0.27.0（メニューは `muda` 0.22.0） | MIT OR Apache-2.0 | tauri-apps。2026-11-25 に更新 | Windows では、作ったスレッドでイベントループが回っている必要がある（README で確認）。バルーン通知の機能はない（ソースで確認） |
| VIVE Hub のプロセスの確かめ（5.6） | `sysinfo` 0.40.4（`default-features = false, features = ["system"]`） | MIT | 2026-11-16 に更新（M2 の設計で確かめた。D-32、`m2-dongle-reader.md` の 10 章） | M2 と同じクレートと決まり。Windows だけの依存にする |
| 状態の窓 | `eframe` / `egui` 0.37.0（内部で `winit` 0.30.13。Apache-2.0） | MIT OR Apache-2.0 | 2026-10-15 に更新 | egui の既定の書体には日本語の字がないので、7.2 と同じく Windows の書体を実行時に読み込む（既定の書体に日本語がないことは、この版では確かめていない）。窓を閉じたときに隠すだけにできるか、トレイと同じイベントループで動くかは、M1-11 の最初に確かめる |
| フォルダーの選択 | `rfd` 0.18.0 | MIT | 2026-11-07 に更新 | |

- 窓は、開いているあいだだけ描く（egui の、入力や更新があったときだけ描き直す動き）。隠しているあいだは描かない（12 章）。
- 候補を比べた結果: `native-windows-gui` は更新が止まっている見込み（この版では確かめていない）、`slint` はライセンスが GPL か独自のもので MIT と組み合わせる条件が複雑、Tauri は WebView を使い M1 の小さな窓には重い。そのため上の組み合わせにした。

### 7.5 UI/UX デザイン担当に頼むこと（PM が `role:ux` の Issue にする）

#27 で直した（PM は #28 にしている）。

1. トレイのアイコンの 3 つの見え方（7.4 の全体: 正常、待機中、異常。灰色のときも待機中の見え方）と、指を置いたときの 1 行。
2. 状態の窓の配置（7.4 の 1〜6。横長。VIVE Hub の配置を参考にする。D-34、D-35）。トラッカーの枠: 呼び名（VIVE Hub の番号とシリアル番号の末尾）、シリアル番号、接続の状態、電池の残り、状態（正常・待機中・異常）、ファームウェアの版、ホストの印を、文章に頼らず配置と印で見せる（W-10、W-11）。読めないときの印（7.4 のトラッカーの項目の表）。3 台でも 5 台でも崩れないこと（W-12）。並びはシリアル番号の順で、呼び名の番号はつながっている台だけに付く（7.4 の「並べ方」）。ドングルの項目。待機中の灰色の見せ方（`greyed`。トラッカーの項目、ドングル、欄を、最後に読めた値として見せる）。「フォルダーを選ぶ」ボタン（`pick_folder`）。VIVE Hub が止まっているときの見せ方（7.4 の「止まったときの見せ方」）。
3. 手引き（G1〜G5）の見せ方: 済んだ段、いまの段、G4 の台数。
4. SteamVR の中の案内の板: 大きさ、HMD からの距離と高さ、表示の長さ（仮 10 秒）、色、字の大きさ。VR の中で読めること（`roles/ux.md` の 4）。
5. デスクトップのトーストの形（見出しと 1 行）。
6. 言い回し: 7.1 の文言、手引きの段、7.4 の「仕組みの側の不具合」の 1 行、呼び名の形（3.2 の表の既定の `トラッカー 1（…0001）` など）。意味は変えない。7.1 の「使わない言葉」を使わない。yomiyasu で推敲する（`roles/ux.md`）。

## 8. 日ごとの集計（W-6）

- 集計（`openut_core::summary`。入出力なし）は、行の時刻と、状態の変わり目と検出から、日ごとに数える。日の区切りと数え方は 8.1。
- **同じファイルの組（前の日のファイルと、その日のファイル）から決まる**ようにする。0 時をまたいで動き続けても、起動し直しても（4.5）、作り直しで再構築しても（4.1）、同じファイルを再生しても（10.2）、同じ結果になる（例外は 8.1 の終わり）。過ぎた日の分は `openut summarize` で作り直せる（M1-9）。
- 書き出しはアプリ（M1-9）: 60 秒ごと、日を閉じたとき、終了のときに、一時ファイルに書いてから名前を変える（途中で落ちても壊れたファイルを残さない）。追いつきのあいだは書かない（4.5）。置き場所は 9.3。

### 8.1 日の区切りと数え方

- **日の区切り**: 日 D の分は、D の 0:00:00.000 から、次の日の 0:00:00.000 の手前まで。比べるのは行に書かれた地方の日付と時刻（`LogTime` では、その行のずれで D の 0 時を表した時刻）。ファイル名の日付では分けない（前の日のファイルに 0 時より後の時刻の行があれば、その行は D の分）。ファイル名の日付は、どのファイルを読むか（4.1、4.5）にだけ使う。
- **時間の値**（`connected_s`、`in_use_s`、`tracking_s`、`unobserved_s`）: サーバーのファイルの、先頭が読めた行（種類を問わない。`Other` も含む。ドライバーの行は時刻を進めないので入れない。3.7）と、次の先頭が読めた行のあいだの区間を、前の行を適用したあとの状態で数える。区間が 0 時をまたぐなら、0 時で 2 つの日に分ける。
  - 区間が `summary.gap_s`（仮 600 秒のまま。7.4 の見張り役の具合 `Stalled` の 10 分と同じ値）より長ければ、その区間は状態に数えず、`unobserved_s` に足す。VIVE Hub が行を書かずに止まったとき（終了の行のない終わり。メモの 3.5）や、PC のスリープ（記録が最長 10.6 時間出なかった）に、止まる前の状態のまま何時間も数えないため。
  - 600 秒の根拠（メモの 3.5）: トラッカーが 1 台以上つながっているあいだのサーバーの記録の間隔は、99% が 8.2 秒以内、99.9% が 270 秒以内。長いほうから 8 つは、8〜10 分が 5 回、20〜36 分が 3 回（後者は、ドングルが抜けたあとなど、つながっていなかった時間を含むかもしれない）。600 秒は、前者を状態に数え、後者を数えない。状態の行は、つながっているトラッカーごとに、変化がなくても数分おきに出る（99% が 687 秒以内）ので、使っているあいだに 10 分を超えて記録が出ないのはまれ。決め直し方: 匿名化した実物のログ（L-2）を `summary.gap_s` を 300・600・1200・1800 秒にして流し、日ごとの `unobserved_s` が、オーナーが使っていなかった時間帯と合う値にする（M1-13 の PR に書く）。
  - 数えるのは**最後に読んだ行の時刻まで**。見張りの時刻（4.6）や再生の `flush_s`（10.2）で時刻を進めても、行のない先の時間は数えない（次の行が来たときに、その区間として数える）。
  - 前の日のファイルがなく、不明な状態から始めたとき（4.5）は、D の 0 時から最初の行までを `unobserved_s` に数える。
- **回数の値**: 出来事の時刻の日に数える（`sessions` は状態が接続に変わった `TrackerAdded`〔重なりは数えない。5.1〕、`power_offs` は R4 の条件 5 で除いた切断、`losses` は追跡中から位置取り直し中に変わった時刻、`first_pose_s` は最初の追跡中になった時刻、`host_starts` は `HostAssigned`）。検出（`detections`、`faults`）は `Raised` の `at` の日に数える。`flush_s` で進めたあいだに出た検出も数える（再生の結果は、最後の行のあと `flush_s` 以上動き続けた見張りと同じ）。
- **日を閉じる**: 最後に読んだサーバーの行の時刻が次の日の 0 時以上になったとき、D を閉じて（`final: true`）、そのときの状態を次の日に持ち越す。ドライバーの行の数（`source.driver`）は、その行の時刻の日に数え、見張りは、D を閉じる前に、ドライバーを D の終わりまで読み終える（4.6 の「ドライバーを待つ」）。実物では、古い日のファイルの最後の記録が、すでに次の日の時刻になっている（4.1。メモの 2.5）ので、D はその記録を読んだときに閉じる。D の後半に行がなければ、次の日の最初の行を読むまで閉じない（VIVE Hub を夜に止めた日は、翌日に使い始めるまで `final: false` のまま）。行が 1 行もない日の集計は作らない。
- **トラッカーの行**: 集計の `trackers` の行は、**シリアル番号ごと**にする（番号はつなぎ直しで変わるため。3.2）。1 回の接続（5.1 の「接続」。状態が接続に変わってから未接続になるまでか、切断の行を見落としたつなぎ直しまで）の時間と回数を、その接続のあいだに分かったシリアル番号の行に足す（`SerialKnown` は、ふつう接続から 1 秒以内に来る。来る前の分も、その接続の分として同じ行に足す）。接続のあいだにシリアル番号が分からなければ、番号ごとの行（`serial: null`）に足す。どちらの行も、その日に使った番号を `slots` に並べる。
- **`tracking_ratio` の `null`**: `in_use_s` が 0 のときの `null` の境界は、集計のテスト（JSON の出力）で確かめる。再生テストの `expect.toml` に `null` の表記は足さない（Issue #20 の PM の判断）。
- **前の日からの持ち越し**: 動き続けているときは、エンジンの状態をそのまま持ち越す。起動し直したとき・再構築したときは、前の日のファイルを先に読んで同じ状態を作る（4.5）。ファイルの日付が飛んだときは、どの場合も不明な状態から始める（4.1）。

**例**（合成。トラッカー 1 は 11/27 の夕方に接続し、追跡中になっている）:

- 11/27 のファイル: …、23:50:00 `Pose data - status: 2`（トラッカー 1、追跡中）、23:59:00 表にない行（11/27 のファイルの最後の行）。
- 11/28 のファイル: 00:04:00 `Pose data - status: 3`（喪失）、00:04:30 `Pose data - status: 2`（追跡中）、00:12:00 表にない行、00:21:00 表にない行、00:30:00 表にない行（最後の行）。
- 行と行の空きは、23:50:00 から順に 540、300（0 時をまたぐ）、30、450、540、540 秒。どれも `summary.gap_s`（600 秒）以内なので、すべての区間を状態に数える。空きが `summary.gap_s` を超える場合は、表のあとに書いた。

| 動かし方 | 11/27 の集計 | 11/28 の集計 |
|---|---|---|
| 11/27 から見張っていて、そのまま 0 時をまたいだ（00:40 の時点） | 23:50:00〜24:00:00 の 600 秒が `tracking_s` に入る。00:04:00 の行を読んだときに閉じる（`final: true`） | `tracking_s` 1770（0:00〜00:04:00 の 240 と、00:04:30〜00:30:00 の 1530）、`in_use_s` 1800、`connected_s` 1800、`losses` 1、`long_losses` 0、`unobserved_s` 0 |
| 00:40 に起動し直した（2 つのファイルがある） | 上と同じ値。`final: true` のファイルがすでにあれば書き直さない（4.5） | 上と同じ |
| 2 つのファイルを再生した | 上と同じ | 上と同じ |
| 00:40 に起動し直した・11/28 のファイルだけを再生した（前の日のファイルがない） | 作らない | トラッカー 1 は 00:04:00 に、推し量った状態（位置取り直し中、`inferred`。5.1）で足される。`unobserved_s` 240（0:00〜00:04:00）、`connected_s` 1560、`tracking_s` 1530、`in_use_s` 1530（最初の追跡中は 00:04:30）、`losses` 0 |

- どの動かし方でも、11/28 の時間の値は 00:30:00（最後の行）まで。00:40 まで行がなくても、再生の `flush_s`（600 秒）を足しても、増えない。
- **空きが `summary.gap_s` を超える場合**（どの動かし方でも同じ）:
  - 11/28 の 00:12:00 と 00:21:00 の行がなければ、00:04:30〜00:30:00 の 1530 秒は `summary.gap_s` を超える空きなので、どの状態にも数えず `unobserved_s` に入る。前の日のファイルがあるとき（表の上の 3 つの行）の 11/28 は、`tracking_s` 240（0:00〜00:04:00）、`connected_s` 270、`in_use_s` 270（00:04:00〜00:04:30 の 30 を足す）、`unobserved_s` 1530、`losses` 1。前の日のファイルがないとき（表の 4 つ目の行）は、`unobserved_s` 1770（0:00〜00:04:00 の 240 と、1530）、`connected_s` 30、`tracking_s` 0、`in_use_s` 0、`tracking_ratio` `null`、`losses` 0。11/27 の集計は、どちらも表と同じ。
  - 11/28 の最初の行が 00:10:00 なら（あとの行もそれより後）、23:59:00〜00:10:00 の 660 秒は `summary.gap_s` を超える空きなので、どの状態にも数えない（11/27 に 60 秒、11/28 に 600 秒の `unobserved_s`）。
- **同じにならない場合**（どれも `source` か、ここに書いた値で分かる）: (1) 切り替わったあとの古いファイルの書き足し（`late_writes`）があった日。動き続けたときは読まず、起動し直すと前の日のファイルとして読む（4.7）。(2) 前の日のファイルの中で、一度も行が出なかったトラッカーの状態（前の前の日に左右される）。(3) `pushes`（そのアプリの回が push した数。起動し直すと 0 から数える。VIVE Hub が止まっているあいだに出さなかった案内は数えない。7.1）。(4) 見張りで、L より遅れて届いた行（サーバーの `time_regressions` と、ドライバーの `driver.late`。4.6）。

形（JSON。`serde_json` 1.0.154、MIT OR Apache-2.0）:

```json
{
  "schema": 1,
  "date": "2026-11-27",
  "final": false,
  "observed_until": "2026-11-27 23:59:00.000",
  "unobserved_s": 0.0,
  "source": { "lines": 0, "meta": 0, "other": 0, "unparsed": 0, "no_header": 0, "bad_offset": 0,
              "replaced_chars": 0, "long_lines": 0, "time_regressions": 0, "late_writes": 0,
              "out_of_order": 0, "rebuilds": 0,
              "driver": { "found": false, "lines": 0, "meta": 0, "no_header": 0, "bad_offset": 0,
                          "replaced_chars": 0, "long_lines": 0, "late": 0, "late_writes": 0 } },
  "trackers": [
    { "serial": null, "slots": [1], "hub_number": null,
      "connected_s": 0.0, "in_use_s": 0.0, "tracking_s": 0.0, "tracking_ratio": null,
      "sessions": 0, "first_pose_s": [], "losses": 0, "long_losses": 0,
      "unexpected_disconnects": 0, "power_offs": 0, "overspeed": 0, "host_count": 0 }
  ],
  "host_starts": 0,
  "faults": { "wifi_info_missing": 0, "unexpected_disconnects": 0 },
  "detections": { "R1": 0, "R2": 0, "R3": 0, "R4": 0, "R5": 0 },
  "pushes": 0
}
```

| 値 | 意味 |
|---|---|
| `final` | 閉じた日（次の日の 0 時以上の時刻の行を読んだ日）なら `true`（8.1） |
| `observed_until` | 時間の値を数え終えた時刻（その日の最後に読んだ行の時刻か、閉じた日なら次の日の 0 時）。ログの地方時 |
| `unobserved_s` | 行が `summary.gap_s` より長く空いた区間と、不明な状態から始めた日の 0 時から最初の行までの時間（8.1）。どの状態にも数えていない |
| `trackers` | シリアル番号ごとの行（8.1）。`serial` が `null` の行は、シリアル番号が分からなかった接続の分で、`slots` の番号ごとに 1 行 |
| `slots` | その行のトラッカーが、その日に使った番号（3.2） |
| `hub_number` | その台の VIVE Hub の番号（3.2。その日の最後の接続で分かった値。5.1）。VIVE Hub は、つなぎ直すたびに振り直しているように見える（U-18）ので、その日の途中で変わっていることがある。分からなければ `null`。初版の `name`（設定のニックネーム）は、ニックネームをやめたので替えた |
| `connected_s` | 未接続以外だった時間 |
| `in_use_s` | 接続してから最初に追跡中になったあと、未接続になるまでの時間（起動直後の待ちを除く） |
| `tracking_s` | 追跡中だった時間 |
| `tracking_ratio` | **位置が出ていた割合** = `tracking_s ÷ in_use_s`。`in_use_s` が 0 なら `null` |
| `sessions` | `TrackerAdded` の回数 |
| `first_pose_s` | 接続（ホストは `HostAssigned`）から最初の追跡中までの秒数。接続のたびに 1 つ |
| `losses` | **喪失の回数** = 追跡中から位置取り直し中への遷移の回数 |
| `long_losses` | そのうち `r5.wait_s` を超えたもの |
| `unexpected_disconnects` | R4 の条件を満たした切断の回数 |
| `power_offs` | 電源オフの予告があったので R4 から除いた切断の回数（6.2 の R4 の条件 5） |
| `host_starts` | `HostAssigned` の回数 |
| `faults` | **不具合の回数**: `wifi_info_missing` は R1 のエピソードの数、`unexpected_disconnects` は R4 の条件を満たした切断の数（全台の合計） |
| `detections` | 規則ごとのエピソードの数 |
| `pushes` | push した案内の数（出しすぎていないかを見るため）。そのアプリの回の数で、ファイルからは決まらない（8.1 の終わり） |
| `source.rebuilds` | 作り直しで再構築した回数（4.1）。アプリの回の数で、ファイルからは決まらない |
| `source.meta`、`source.bad_offset` | `#` で始まる行の数、ずれが読めなかった行の数（4.3、4.7） |
| `source.driver` | ドライバーのファイルの数（4.7）。`found` は、その日にドライバーのファイルを読めたか（`false` なら R4 の条件 5 を確かめていない）。`late` は L より遅れて届いた記録（4.6。見張りの回の数で、再生では 0） |

- `serial` と `hub_number` は、分かったときだけ。集計にはシリアル番号が入るので、利用者の PC にだけ置く。送る機能（L-4）では、匿名化してから送る。
- 「位置が出ていた割合」は、`findings-2026-11-28.md` の「位置が出ていたフレームの割合」とは数え方が違う（あちらはトラッカー内の毎フレームの記録、こちらは PC のログの状態の変わり目）。突き合わせはまだしていない。

## 9. 設定と保存場所（W-9）

### 9.1 設定のファイル

- 置き場所: `%APPDATA%\OpenUT\config.toml`（`--config` で変えられる）。なくても動く（すべて既定値）。読めない・形が違うときは、既定値で動き、アプリのログに理由を書く（止まらない。画面には出さない。W-10）。
- 形は TOML（`toml` 1.2.3、`serde` 1.0.229。どちらも MIT OR Apache-2.0）。アプリが書き直すとき（通知の切り替え、フォルダーの選択）は、ファイル全体を書き直す（コメントは残らない。仕方ないものとし、ファイルの先頭にそう書く）。

```toml
# OpenUT の設定。OpenUT が書き直すことがある（コメントは消える）。

# ログのフォルダー。書かなければ自動で探す（9.2）。
# log_dir = 'D:\path\to\logs'

[notify]
steamvr = true
desktop = true

# VIVE Hub のトラッカー用サーバーのプロセスの確かめ（5.6）。名前を書かなければ ViveTrackerServer.exe だけ。
# [watch]
# hub_process_names = ["ViveTrackerServer.exe"]
# process_check_s = 10   # プロセスを確かめる間隔（5.6）
# start_wait_s = 60      # 起動し直した VIVE Hub の開始の行を待つ長さ（5.6）。仮

# しきい値（6 章）。書かなければ既定値。上級者向け。
[rules]
# r1.count = 3
# r1.span_s = 15
# r1.gap_s = 10
# r3.clients = false   # PM の判断のあとで既定を決め直す（6.2）
# r4.exclude_power_off = true   # オーナーの確かめのあとで既定を決め直す（6.2）
# r5.enabled = false   # PM の判断のあとで既定を決め直す（6.2）

# 集計（8 章）
[summary]
# gap_s = 600
```

- 名前の書き方は、`--set` と `expect.toml` と同じ（`rules.r1.count`、`rules.r3.clients`、`summary.gap_s`、`notice.cooldown_s` など）。

- **台数とシリアル番号を固定しない**: 状態はログに出てきた番号から作り（5.1）、画面に並べる台はログに出てきたシリアル番号から作る（5.4）。設定に、台数もシリアル番号も書かない。
- **トラッカーの名前の設定はない**（#27。W-12）。初版の `[trackers."<シリアル番号>"] name = "腰"`（ニックネーム）は、部位の名前を付ける前提だったので、やめた。呼び名は、VIVE Hub の番号とシリアル番号の末尾で決まる（3.2）。初版の設定のファイルに `[trackers]` があっても、読み飛ばす（止まらない。アプリのログに 1 回だけ書く）。

### 9.2 ログのフォルダーを探す

順に試し、`ViveTrackerServer_*.txt` があるフォルダーを使う。

1. 設定の `log_dir`、またはコマンドの `--log-dir`。
2. 既知の候補: `%ProgramData%\HTC\ViveSoftware\ViveUltimateTracker\Log`（メモの 1 章。VIVE Hub の 1 つの版で確かめた場所。利用者のフォルダー名を含まない）。`%ProgramData%` は環境変数から作る（変数がなければ、この候補を飛ばす）。候補を足すときも環境変数からの相対で書き、利用者のフォルダー名を設計にもコードにも書かない。ドライバーのファイルは、見つけたフォルダーと同じ場所で探す（4.1）。
3. 見つからなければ、見張り役の具合を `NotFound` にする（7.4 の「仕組みの側の不具合」。利用者には手引きの G1 と「フォルダーを選ぶ」〔F4〕で見せる）。

- 候補の場所を、深くたどって探し回ることはしない（起動を遅くしないため）。
- 見つけたフォルダーの場所は、アプリのログにだけ書く（利用者のフォルダー名が入るため、集計には書かない）。

### 9.3 OpenUT が書く場所

| もの | 場所 |
|---|---|
| 設定 | `%APPDATA%\OpenUT\config.toml` |
| 日ごとの集計 | `%LOCALAPPDATA%\OpenUT\summary\YYYY-MM-DD.json`（`final: true` のものは、起動し直したときに書き直さない。4.5） |
| 作り直しの前の集計 | `%LOCALAPPDATA%\OpenUT\summary\YYYY-MM-DD.before-rebuild-N.json`（N は、その日の 1 からの通し番号。4.1） |
| アプリ自身のログ | `%LOCALAPPDATA%\OpenUT\logs\openut-YYYYMMDD.log`（UTF-8。14 日より古いものは起動時に消す） |

- 場所は環境変数から作る（クレートを足さない）。`--data-dir` で変えられる（テストで使う）。
- VIVE Hub のフォルダーには何も書かない。

## 10. 再生テスト（W-8）

### 10.1 入力の置き場所と形

```
tests/replay/<ケースの名前>/
  ViveTrackerServer_YYYYMMDD.txt   … 入力のログ。1 つ以上。名前の日付の順に流す
  ViveTrackerDriver_YYYYMMDD.txt   … 任意。ドライバーのログ（4.1）。サーバーと時刻の順に合わせて流す（10.2）
  expect.toml                      … 期待する検出と集計
  README.md                        … 任意。何を確かめるケースか（1〜3 行）
```

- 入力のログの文字コードと改行の型は、4.2 の決め方で読む。実物と同じ UTF-16LE（BOM つき、CRLF）でも、読みやすい UTF-8（LF）でもよい。合成のケースは UTF-8 と LF で書いてよい（差分が読める）。UTF-16 と CRLF の読み取りそのものは、解読の単体テスト（M1-1。本文の中の LF を含む）と、UTF-16 で保存した合成のケースを 1 つ置いて確かめる。
- **合成のケースの行は、見本（`docs/research/hub-log-samples.md`）の形で書く**（関数名、レベル、本文の綴り。番号のない行には番号を付けない）。ファイルの先頭は、実物と同じく `# Log begin at ...` と設定の行、`Log Start`、`Tracker Server Starts.` で始める（見本の 1 章）。スレッド番号と時刻は合成でよい。シリアル番号は見本の置き換えの形（`00XX000000001` など）にする。
- 実物のログは、匿名化の道具（L-1）を通したものだけを入れる（PRD の L-2）。トラッカーの名前は道具の置き換え（例: `TRACKER-A`）のまま。
- 最初に置く合成のケース（M1-2）:
  - `synthetic-r1-second-host-command`: 2 台目のトラッカーが登録される直前に 2 度目の `Tracker (N) - switch to HOST mode.` が出て、そのあと `Wifi info is not yet available.`（番号なし、`WARN`）が 0.55 秒おきに 60 秒続く（見本の 3 章の経過を、短くしたもの）。R1 が 1 回（案内するのはホスト）。
  - `synthetic-normal-start`: `Add new tracker`、ホストへの `switch to HOST mode.`、`Query wifi from tracker N`、`Got host wifi info.`、2 台目の `Add new tracker`、`Assign wifi info to client tracker (index: N).`、`Client tracker (index: N) is connected to host: 1`、2 台目の状態の行（`map status: 6`、`map state: 2`）、姿勢の行（`status: 2`）の順のふつうの開始。`GetDeviceInfoAndResponse` の行（置き換えたシリアル番号）を含める。検出なし。
  - `synthetic-utf16`: `synthetic-normal-start` を UTF-16LE（BOM つき、CRLF）で保存したもの。結果が同じこと。
  - M1-2 で入れた 3 つのケースの行は、初版の語句で書いてある。見本の形に直すのは、実装計画の 3 章の F-3。
- 規則と集計の項目で足すケース:
  - `synthetic-r1-gap-then-fault`（M1-6）: ふつうの開始の途中で、断られた問い合わせが 2 回、61 秒空いて 1 回（ここでは出ない）。そのあとクライアントが外れて R1 の条件がなくなり、しばらくして 2 度目の `switch to HOST mode` から `Wifi info is not yet available.` が 0.55 秒おきに 60 秒続く（ここで出る）。R1 がちょうど 1 回（6.2 の R1 の例）。
  - `synthetic-midnight`（M1-8）: 8.1 の例の 2 つのファイル。`expect.toml` の `[summary."2026-11-27"]` と `[summary."2026-11-28"]` に 8.1 の表の値。
  - `synthetic-midnight-no-prev`（M1-8）: 同じ例の 11/28 のファイルだけ。8.1 の表の「前の日のファイルがない」の値。
  - `synthetic-r4-power-off`（M1-6）: 3 台がつながって 1 分以上たってから、(a) 1 台が、ドライバーのファイルの `(N) Tracker is going to power-off.` と同じ時刻に `Remove tracker (N), ...` を 2 行出して切れる（R4 は出ない。`power_offs` が 1）、(b) 別の 1 台が、予告なしで切れる（R4 が 1 回）、(c) 残りの 1 台が `connection type: 2` で切れる（R4 は出ない）。(a) の予告の行の時刻は、実物と同じく切断の行と同じミリ秒にする。ドライバーのファイルが遅れて届いても（例: サーバーより 3 秒あとに追記される）、L 以内なら同じ結果になること（4.6 の「ドライバーを待つ」）は、見張りのループのテスト（M1-9）で、このケースのファイルを時間差で伸ばして確かめる。
- 実物のログを入れるケースの名前は `hub-<最初の日付>-<最後の日>`（例: `hub-2026-11-23-27`）。サーバーとドライバーの両方のファイルを入れる。2026-11-22 のファイルは、VIVE Hub が消したので残っていない（メモの 1 章。過去 5 日分しか残らない）。受け入れの基準（PRD の 5 章の「2026-11-22〜27」）の読み方は PM が決める（14 章）。大きさによっては、レポジトリへの入れ方も PM が決める。

### 10.2 時刻の進め方

- 入力のファイルを、種類（サーバー、ドライバー）ごとに名前の日付の順につなぎ、2 つの列で 1 つの時間の基準でずれを補い（4.3。サーバーの列の最初の記録で基準を決めてから、両方の列を直す）、2 つの列を、それぞれの中の順を保ったまま時刻の順に合わせて（同じ時刻ならサーバーを先。4.6）流す。壁時計は使わない。ドライバーの記録は時刻を進めない（3.7）。時刻の順に流すので、`driver_needed_through` の待ち（4.6）は起きない（期限より後のサーバーの記録の前に、期限までのドライバーの記録がすべて入っている）。
- 出来事ごとに、エンジンの `apply`（中で、その時刻まで `advance_to`）を呼び、ほかの先頭が読めた行は `note_line` を呼ぶ。最後の行のあと、`advance_to(最後の行の時刻 + flush_s)`（既定 600 秒。`expect.toml` で変えられる）を呼んで、残っている期限を判定する。`flush_s` は期限の判定のためだけに時刻を進め、集計の時間の値には足さない（8.1）。結果は、最後の行のあと `flush_s` 以上動き続けた見張りと同じ。
- 日の区切りは、行の時刻で、見張りのときと同じに起きる（8.1）。ファイルの日付が飛んでいれば、見張りのときと同じに `Engine::rebuild` で初期の状態に戻す（4.1）。最初のファイルは、前の日のファイルがない起動と同じ（不明な状態から始まる。4.5）。
- 同じ入力を、見張りのように 500 ms ごとの `advance_to` を挟んで流しても、同じ検出と集計になる（3.7）。M1-6 と M1-9 で、これをテストで確かめる。
- 再生の最初のファイルは「追いつき」として扱わない（`[replay] catch_up = true` で、ファイルの最後までを追いつきとして流すこともできる。4.5 の確かめに使う）。
- 再生の本体（入力のバイト列を受け取り、出力を返す）は `openut_hublog::replay` に入出力なしで書く。ファイルを読むのは、テストと `openut replay` コマンド。

### 10.3 期待する検出の書き方（`expect.toml`）

```toml
# この一覧にある規則は、下に書いた検出のほかに出たら失敗にする。
check_rules = ["R1"]

[replay]
flush_s = 600          # 任意
# catch_up = false     # 任意

[rules]                # 任意。しきい値の上書き（9.1 と同じ名前）
# r1.span_s = 15
# r5.enabled = true    # 既定で切ってある規則を確かめるケースで

[[detection]]
rule = "R1"
tracker = 2                          # 任意。番号（3.2）
after = "2026-11-26 01:43:00"        # この時刻以後（ログの地方時）
before = "2026-11-26 01:45:00"       # この時刻より前

[summary."2026-11-26"]               # 任意。集計（8 章）の値の一部
"faults.wifi_info_missing" = 1
```

- 比べ方: `check_rules` の規則について、実際の `Raised` と、期待の `[[detection]]` を、時刻の順に 1 つずつ組にする（`rule` が同じで、`after` 以上 `before` 未満で、`tracker` を書いたなら番号も同じもの）。組にならない期待は「見つからない」、組にならない実際の検出は「余分」として、どちらも失敗にする。`check_rules` にない規則の検出は、結果に表示するだけ。
- 失敗のときは、期待と実際の一覧を、時刻と規則とトラッカーの番号で並べて出す（どこが違うか読めるように）。
- `[summary]` は、書いた値だけを比べる（キーは 8 章の JSON の場所を `.` でつないだもの）。
- 実際の検出の一覧は、`openut replay <ケース> --format toml` で、この形のまま出せるようにする（期待を書き始めるときに使う。出したものを、人が根拠と照らしてから期待にする）。

### 10.4 受け入れの基準のテスト

`tests/replay/hub-<日付の範囲>/expect.toml`（M1-13。PM が匿名化したログを入れたあと。名前は 10.1）:

- `check_rules = ["R1"]`。
- `[[detection]]` を 4 つ: 2026-11-26 01:43、2026-11-27 22:53、22:56、22:59。それぞれ `after` をその分の 0 秒、`before` を 2 分後にする（R1 は不具合の始まりから `r1.span_s` 秒あとに出るため）。2 分で足りなければ、しきい値の決め方（6.1）で決めた値に合わせて広げ、理由を PR に書く。

## 11. 実機に送る命令（W-7、D-13）

**M1 が送る命令: なし。**

| 相手 | 送るもの | 理由 |
|---|---|---|
| ドングル（USB HID） | なし | M1 は HID を開かない。M1 の依存に HID を扱うクレートを入れない |
| トラッカー | なし | 同上 |
| VIVE Hub のサーバー（名前付きパイプ） | なし | パイプを開かない |
| VIVE Hub のログ | なし（読み取り専用で開く。4.2） | |
| SteamVR | オーバーレイの表示だけ（7.2） | トラッカーの機器ではない。SteamVR の設定や、ほかの機器の状態は変えない |
| Windows のプロセス（`ViveTrackerServer.exe`） | なし（プロセスの一覧の名前を読むだけ。5.6） | VIVE Hub が動いているかを見るため（手引きの G1）。プロセスを開かず、止めず、何も送らない |

- 立て直しの命令（D-13）は M3 で、M2 の命令の一覧（Issue #6）に入ってから。
- レビューでは、M1 の PR に、HID・名前付きパイプ・ドングルを扱うコードや依存が入っていないことを見る。

## 12. 性能（VR の動作を重くしない）

- 見張りは 500 ms ごとに、フォルダーの一覧と 2 つのファイル（サーバー、ドライバー）の大きさを見て、伸びた分だけを読む。VIVE Hub のプロセスの確かめ（5.6）は、起動したときと 10 秒ごと（と、止まっているあいだにサーバーのファイルが伸びた回）に、プロセスの名前の一覧だけを取る（`sysinfo` の、プロセスの名前だけを更新する呼び方。M1-9 で、その呼び方を docs.rs で確かめ、かかる時間を PR に書く）。1 行の判定は、本文の先頭を数回比べるだけ（4.4）。
- SteamVR の中の表示は、案内を出すときだけ画像を渡す。毎フレームの描画はしない（7.2）。状態の窓は、隠しているあいだ描かない（7.4）。
- 起動時の追いつきは、1 回 4 MiB ずつ読む（4.2）。1 日分の大きさ（メモの 1 章）: サーバーは 2,002〜15,828 行、0.7〜5.5 MB。ドライバーは 1,957〜18,775 行、0.5〜4.6 MB（9 割以上が `Wait for HID data got timeout.`）。追いつきで読むのは 2 日分の 2 種類で、多くて約 20 MB（5 回ほどの読み取り）。使っているあいだは、合わせて毎秒 1 行に満たない。
- 目安（測っていない。M1-9 で、合成の大きなログで測って PR に書く）: 見張り中の CPU は 1 つのコアの 1% 未満、メモリは 50 MB 未満（状態の窓を開いていないとき）。
- オーナーの実機での確認（M1-12 のあと。PM が `needs-human` にまとめる）: VIVE Hub と SteamVR を動かしてフルトラッキングで 10 分遊び、タスクマネージャーで OpenUT の CPU とメモリを見る。SteamVR のフレームの時間（SteamVR の「詳細設定」の「フレームタイミングを表示」）を、OpenUT を動かす前と後で見比べる。

## 13. 使うクレートと API（まとめ）

版と更新日は、2026-11-28 に crates.io の API で確かめた。ライセンスは、どれも MIT と組み合わせられる（Apache-2.0 のものは、配布するときにその条件の表示が要る）。

| 用途 | 使うもの | 版 | ライセンス | 保守（最終更新） | 段階 |
|---|---|---|---|---|---|
| UTF-16 の解読、ファイル、時刻、スレッド | 標準ライブラリ | Rust 1.100.0 | — | — | M1-1〜 |
| 設定、期待のファイル | `serde`、`toml` | 1.0.229、1.2.3 | MIT OR Apache-2.0 | 2026-07、2026-11 | M1-2、M1-9 |
| 集計のファイル | `serde_json` | 1.0.154 | MIT OR Apache-2.0 | 2026-10 | M1-8 |
| デスクトップの通知 | `tauri-winrt-notification` | 0.8.1 | MIT OR Apache-2.0 | 2026-07 | M1-10 |
| トレイ | `tray-icon`（`muda`） | 0.27.0（0.22.0） | MIT OR Apache-2.0 | 2026-11 | M1-11 |
| 状態の窓 | `eframe`、`egui`（`winit`） | 0.37.0（0.30.13） | MIT OR Apache-2.0（winit は Apache-2.0） | 2026-10 | M1-11 |
| VIVE Hub のプロセスの確かめ | `sysinfo` | 0.40.4 | MIT | 2026-11（M2 の設計で確かめた。D-32） | M1-9 |
| フォルダーの選択 | `rfd` | 0.18.0 | MIT | 2026-11 | M1-11 |
| SteamVR | `openvr`（`openvr_sys`） | 0.9.0（2.1.4） | MIT（OpenVR は BSD 3 条項の形） | 2026-04。「passively-maintained」の印 | M1-12 |
| SteamVR の代案 | `libloading` | 0.9.1 | ISC | 2026-11 | M1-12 |
| 文字の描画 | `fontdue` か `ab_glyph` | 0.9.5、0.2.32 | MIT OR Apache-2.0 OR Zlib、Apache-2.0 | 2026-10、2025-09 | M1-12 |

使わないと決めたもの: `notify` 9.0.0（CC0-1.0。ファイルの変更の通知。4.1 の理由）、`regex`（4.4 の理由）、時刻のクレート（`jiff`、`time`、`chrono`。3.1 の理由）、非同期のランタイム（2 章の理由）、引数の解析のクレート（サブコマンドが少ないので、標準ライブラリで書く。増えたら `lexopt` 0.3.2〔MIT〕を考える）。

docs.rs と Microsoft の文書は、この環境から読めなかった（接続を断られた）。API の確かめは、crates.io から取ったクレートのソースと、ValveSoftware/openvr のヘッダーで行った。実装のときに docs.rs で確かめ直す。

## 14. 未確認・判断を仰ぎたい点

**PM に判断を仰ぎたい点**

判断が出るまでは、既定を W-4 のとおりにしてある（R3 はホストだけ、R5 は切ってある。6.2）。認められたものだけ、設計の追補で既定を変える（実装は既定の値を変えるだけで済むように作る。実装計画の 3 章）。

1. **R5（使用中の長い喪失）を足してよいか。** W-4 の 3 つに入らない、足した規則（6.2）。既定は `rules.r5.enabled = false`。判断の材料は、実物のログを `--set rules.r5.enabled=true` で再生したときの数（M1-13 の PR に書く）。
2. **R3 をホスト以外にも当ててよいか。** W-4 は「ホストが位置を出せない」。既定は `rules.r3.clients = false`（ホストだけ）。当ててよければ `true` にする（位置合わせの待ちはどのトラッカーにも起き、案内も同じため。6.2）。
3. **実物のログの入れ方と、受け入れの基準の読み方。** 匿名化したあとの大きさによっては、レポジトリに直接入れるのが重い。圧縮して入れるか（`.gitignore` に `*.tgz` がある）、一部の時間帯だけ切り出すか（受け入れの基準は 6 日分を流すことなので、切り出すなら基準の読み方も決める）。**2026-11-22 のファイルは残っていない**（メモの 1 章）。受け入れの基準（PRD の 5 章。11/22〜27 で W-3 が 4 回だけ）を、残っている 11/23〜27 で確かめてよいか（4 回の時刻は、どれも 11/26 と 11/27）。ドライバーのファイルも一緒に入れる（R4 の条件 5。10.1）。
4. **ドライバーのファイルも読んでよいか。** PRD の W-1 は、サーバーのファイル（`ViveTrackerServer_YYYYMMDD.txt`）を読むと書いている。この版は、電源オフの予告のためだけに、ドライバーのファイルも読む（4.1）。読むだけで、W-7 は変わらない。W-1 の書き方を直すかは PM が決める（設計は PRD を変えない）。認められなければ、R4 の条件 5 を外す（電源を切るたびに案内が出うる）。
5. **R4 で、電源オフの予告のあった切断を、既定で除いてよいか。** 既定は `rules.r4.exclude_power_off = true`（仮。6.2 の R4）。利用者の操作によるものかは、オーナーの試し（下の 2）を待つ。除かないと、使い終わって電源を切るたびに、台ごとに push される。
6. **（PR #37 のレビューの指摘で、問いを改めた）確かめられないときは、全体を待機中の灰色で見せる**（#27。W-10）。初版は、追いつきのあいだと、見張り役の具合が `Unreadable`・`Stalled`・`FormatMismatch` のあいだ、全体を 4 つ目の「確かめられない」にしていた。W-10 の (b) は 3 つ（正常・待機中・異常）なので、この版は 3 つのままにし、そのあいだは待機中にして、トラッカーの項目・ドングル・欄を灰色（最後に読めた値。`greyed`）で見せ、すること 1 行を添える（7.4 の「全体」）。正常や待機中と同じに見えて、利用者が OpenUT を当てにしてしまうおそれは、灰色とすること 1 行で抑える。**4 つ目の見え方が要ると PM が判断するなら**、先に PRD（W-10）を直してから、設計の追補で足す（`Overall` に足し、トレイの見え方を 4 つにする。M1-7 と M1-11 のテストも直す）。
7. **電池が少ないときに、案内を出すか**（W-11 は見せるだけ）。この版は案内を作らない（7.4）。出すなら、しきい値（例: 15%）と、push するかを決めて、規則として足す（設計の追補）。
8. **VIVE Hub のプロセスの一覧を読んでよいか**（#27。5.6）。W-1 はログを読むことを書いている。この版は、VIVE Hub が終了の行なしで止まったことを見分けるために、Windows のプロセスの名前の一覧も読む（M2 の DG-3 と同じ `sysinfo`。読むだけで、W-7 は変わらない。11 章）。この結果は、画面のほか、止まっている・起動の途中のあいだに案内を出さないこと（7.1。出口の見張り。5.6）にも使う。認められなければ、ログだけで決める（`HubLive::Unknown`。終了の行なしで止まり、トラッカーがつながっていないときは、手引きが G3「電源を入れる」のままになる。止まる前に始まった待ちの案内〔R2 など〕は、止まっているあいだに期限が来ると push されうる）。

**オーナーに確かめてもらいたいこと**（PM が `needs-human` にまとめる。どれも 10 分以内）

1. **書いている最中のログを読めるか（U-13）と、書かれる間隔（U-8）。** VIVE Hub を起動してトラッカーを 1 台つなぎ、メモ帳で `%ProgramData%\HTC\ViveSoftware\ViveUltimateTracker\Log` の `ViveTrackerServer_<今日の日付>.txt` を開く。開けるか、開けたら「ファイル → 開く」でもう一度開いたときに行が増えているかを書く。続けて、PowerShell で `1..30 | % { (Get-Item "$env:ProgramData\HTC\ViveSoftware\ViveUltimateTracker\Log\ViveTrackerServer_<今日の日付>.txt").Length; Start-Sleep 1 }` を動かし（トラッカーを動かしながら）、出た 30 個の数をそのまま貼る（大きさが変わる間隔が、まとめて書かれる間隔）。開けなければ、設計を直す（4.2）。
2. **電源オフの予告が、ボタンで電源を切ったときに出るか（U-11。R4 の条件 5 の根拠）。** トラッカーを 3 台つないで 1 分待ってから、1 台ずつ、電源ボタンで電源を切る。それぞれ切った時刻（時計で分と秒）を控え、30 秒あける。終わったら、その日の `ViveTrackerDriver_<今日の日付>.txt` で `Tracker is going to power-off.` の行の時刻と、控えた時刻を並べて書く（ファイルは貼らない。時刻と番号だけ）。3 回とも 10 秒以内に合えば、予告は利用者の操作の印として扱う。合わなければ、R4 の既定を見直す。
3. **ホストの電源の入れ直しで、「Wi-Fi 情報なし」が直るか（R1 の案内の根拠）。** 次に「始まらない」が起きたときに、ドングルのリセットをせず、ホストのトラッカーの電源だけを入れ直し、ほかのトラッカーが使えるようになるまでの時間と、そのときのログを残す。ホストは、サーバーのファイルの最後の `Tracker (N) - switch to HOST mode.` の N（いまの番号）。直らなかったら、どのくらい待ったかを書く。
4. **性能（12 章）。** M1-12 のあと。
5. **VIVE Hub の画面との見比べ（U-18〜U-20。#27）。** M1-11 のあと。VIVE Hub と OpenUT の状態の窓を並べて開き、トラッカーを 3 台つないで追跡を始める。台ごとに、番号、シリアル番号の末尾、電池の残り、ファームウェアの版、ホストの印が、VIVE Hub の画面と合うかを書く（シリアル番号は貼らない。合う・合わないだけ）。VIVE Hub の画面の台の並びが、番号の順か、ほかの順かも書く。続けて、ホストの電源を入れ直して、ホストの印が移るか、つなぎ直したあとの番号が、VIVE Hub の画面と OpenUT で同じかを書く（番号は、つなぎ直しで変わることがある。U-18）。最後に、ドングルを抜いて 1 分待ち、OpenUT のドングルの項目が「つながっていない」になるかを書き、挿し直す。
6. **手引きの段の順（7.4。G4 の「追跡を待つ」が、G5 の「身につける」より前）。** UI/UX デザインの確認（#23 と同じ形）で、オーナーに見てもらう。ふだんの使い方と違えば、順を直す（設計の追補）。
7. **VIVE Hub を止めて起動し直したときの見せ方（5.6、7.4 の「全体」の 1・5。PR #37 のレビュー〔2 回目〕）。** M1-11 のあと。トラッカーを 2 台以上つないで追跡を始め、タスクマネージャーで `ViveTrackerServer.exe` を終了する（終了の行のない止まり方の試し）。20 秒以内に、OpenUT の窓が「VIVE Hub を起動する」になり、台がすべて未接続になり、VR の中とデスクトップに案内が出ないかを書く。1 分待ってから VIVE Hub を起動し、窓に止まる前の追跡中の台が戻らないまま、台がつながり直すかと、プロセスを起動してから台が出るまでの秒数（時計で）を書く（`watch.start_wait_s` を決めるため）。

**確かめていないこと**（設計に書いた見込み。実装と再生テストで確かめる）

- ログの書式の残り（4.8 の U-3、U-5、U-8、U-9、U-11、U-12、U-13、U-17）。
- `VRApplication_Background` でオーバーレイを作れるか、`openvr_sys` が CI の Windows でビルドできるか（7.2）。
- トーストの送り主の ID の登録の方法、トーストを押したことを受け取れるか（7.3）。
- 日本語以外の Windows にある書体（7.2）。
- egui の窓を、トレイと同じイベントループで、閉じても終わらずに動かせるか（7.4）。
- R1 の `span_s` を 15 秒、`gap_s` を 10 秒にしてよいか（6 日分では、ふつうの開始で断られる問い合わせは出ていないが、ほかの環境では分からない）。R2〜R5 のしきい値（6.1 の決め方で決める）。R4 の `notice_before_s`、`notice_after_s`（予告が切断より後に出るか。6.2）。重なりの `state.dup_ms`（1 秒。`Remove tracker` の 2 行は 3 ミリ秒おきだったが、`Add new tracker` の重なりは見本にない）。
- 集計の `summary.gap_s` を 600 秒にしてよいか（8.1 の決め直し方）。
- 見張りの余裕 L を 10 秒にしてよいか（4.6。オーナーの試しの 1）。追いつきを読み終えたとき、サーバーの最後の記録より L 以上前の時刻のドライバーの記録は、書き終わっていると見なしてよいか（4.6 の 3。2 つのファイルを同じプロセスが同じ間隔で書くことからの見込み。M1-9 のアプリのログの、まとまりごとの遅れで確かめる）。
- ずれの読めない行が、夏時間の変わり目の近くにあったときの時刻（4.3。6 日分では、ずれの読めない行はない）。
- 切断の行を見落としたつなぎ直し（5.1）が、実物で起きるか（6 日分の `Add new tracker` の重なりは数えていない。M1-13 で「順序の合わない出来事」の数を見る）。
- VIVE Hub の番号（`model name` の N）が VIVE Hub の画面の番号と同じか（台ごとに固定でないことは分かった）、電池（`percentage`）、ドングル（`dongle connected`）の値の意味（4.8 の U-18〜U-20）。
- `sysinfo` で、プロセスの名前だけを 10 秒ごとに取る呼び方と、かかる時間（5.6、12 章。M1-9 で docs.rs を見て確かめる）。
- 起動し直した VIVE Hub が、開始の行をファイルに出すまでの時間（5.6 の `watch.start_wait_s`。仮 60 秒。M1-9 のアプリのログと、オーナーの確かめの 7）。止まったプロセスの最後の書き込みが、止まってから 1 回の読み取りのうちに読めること（5.6 の「確かめ直し」で、名前が合っていないとみなすまでの `watch.process_check_s`）。PC の終了やスリープで止まったときも、プロセスの一覧から消えるか（スリープでは、プロセスは残る見込み。そのときは、ログが伸びないことの `Stalled` で分かる）。
