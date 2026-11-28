# M2 の道具の設計: ドングルの読み取り（読むだけ）

VIVE Hub のトラッカー用サーバー（`ViveTrackerServer.exe`）を止めた状態で、Windows から、ドングルにつながったトラッカーの状態と姿勢を**読むだけ**の道具（名前は `openut-dongle`）の設計。PRD の 7 章（DG-1〜DG-4）、Issue #6。通信の根拠は `m2-dongle-protocol.md`（根拠の記号 S1〜S5、R1 と、分からないことの番号 U-1〜U-17 もそこにある）、送りうる命令は `commands.md`。

## 1. 目的と範囲

- **やること**: ドングルを見つけて開き、受け取った報告（入力レポート）を、時刻つきでそのまま記録しながら解読し、トラッカーごとの状態と姿勢をコンソールに出す。記録は、あとから解読し直せる形にする（M3 の再生テストの入力。DG-4）。
- **やらないこと**: ドングルとトラッカーに、何も送らない（DG-2）。機能レポートも読まない（Windows の `HidD_GetFeature` も機器への要求になるため）。SteamVR に渡さない。画面（窓）を持たない。
- **前提の確認**: 「ドングルは同時に 1 つのプログラムしか扱えない」という前提（DG-3）を確かめる試しを、道具の機能として持つ（7.3）。
- この道具で分かったことを使って、M3 の置き換えランタイムを作る。M2 で作る部品（記録、解読、トラッカーの状態表）は、M3 でそのまま使う。

## 2. 要件との対応

| 要件 | どこで満たすか |
|---|---|
| DG-1 状態（接続、役割、地図の状態、追跡の状態）と姿勢を読む | 解読（6 章）、トラッカーの状態表（6.4）、コンソールの表示（9.2）。**頼まずに届くかは未確認**（`m2-dongle-protocol.md` の U-1）。届かない場合の扱いは PR の「未解決」で PM に聞く |
| DG-2 命令を送らない。読むために送るものがあれば一覧にして了承を得る | 送る部分を持たない作りと、その検査（4 章）。問い合わせの候補 Q1〜Q3 は `commands.md` の 2 章。M2 の最初の実装には入れない |
| DG-3 VIVE Hub のサーバーが動いていたら断って理由を示す。前提も確かめる | サーバーの見張り（7 章）。前提の確認の試し（7.3、試し B） |
| DG-4 読んだ値を時刻つきで記録できる | 記録の形（8 章）。共有用の書き出し（8.4。解読して安全と判断できた欄だけで作り直す） |
| M2 の終わりの目安「3 台の状態と姿勢が読め、VIVE Hub のログの値と合う」 | 要約（9.1 の `summary`）と、試し B・C の記録を VIVE Hub のログ（#7 の書式のメモ）と突き合わせる（11 章） |
| PRD 11 章（危険な命令を送らない、個人の情報を出さない、他人のコードを写さない、UI は日本語） | 4 章、8.3・8.4、`m2-dongle-protocol.md` の冒頭、9.2 |

## 3. 構成

判定と解読は、入出力から分ける（AGENTS.md）。Windows の HID とプロセスの一覧に触る部分は、薄い 2 つの部品（`HidReportSource`、`ProcessList`）だけにする。

```
                 ┌──────────────── Windows だけ ────────────────┐
  ドングル ──→   │ HidReportSource（hidapi。読むだけの包み）       │──┐
                 │ ProcessList（sysinfo。プロセスの名前の一覧）    │  │
                 └──────────────────────────────────────────────┘  │ RawReport
                                                                    ▼
  記録ファイル ─→ RecordReader（どの OS でも） ───────────────→  ┌──────────────┐
                                                                  │ Decoder（純粋）│→ DongleEvent
                                                                  └──────────────┘
                                                                          ▼
  RecordWriter ←── RawReport（受け取ったまま）              TrackerTable（純粋）→ TrackerView
                                                                          ▼
                                                          View（純粋。表示の文字列を作る）→ コンソール
  ServerGuard（純粋。プロセスの名前の一覧 → 断るか）
  Anonymizer（純粋。記録 → 許した欄だけで作り直した記録 ＋ 残っていないかの確認）
  Clock（単調な時計。読み取りのスレッドだけが読む。10 章）
```

- 再生（記録ファイルからの読み取り）と実機からの読み取りは、同じ `RawReport` と、同じ「時刻を進める」入力（`TrackerTable::advance`）を流す。解読から先は、どちらでも同じコードが動く（M3 の再生テストの土台）。
- **時刻は呼ぶ側が渡す。** 純粋な部品は時計を読まない。時刻は、報告に付けた単調な時刻（`mono_us`）と、報告が来ないあいだの `advance(now_us)` で進む。これで、全台の報告が止まっても途切れの判定が進む（6.4）。
- **時刻の流れは 1 本にする（順序の決まり）。** 実機では、**読み取りのスレッドだけが時計を読み**、受け取った報告と、受け取りの待ちの時間切れ（100 ms）の印（`Idle`）を、時計を読んだ順に 1 本の流れ（`SourceItem`。5 章）にして主のスレッドへ渡す。主のスレッドは、その順に `apply` と `advance` を呼ぶだけで、状態表を進めるために時計を読まない。こうすると、`advance(t)` を呼ぶ時点で、時刻が t までの報告はすべて当ててある（主のスレッドの処理が遅れて、流れがたまっていても同じ）。記録の行も、この流れの順と時刻で書くので、再生は記録の行の順に同じ呼び出しをすることになり、実機と再生で同じ結果になる（6.4、10 章）。

## 4. 「送らない」の保証

Windows では、読み取りの権限だけで開いても機能レポートは送れる（`m2-dongle-protocol.md` の 7 章）。OS の権限では保証できないので、作りと検査で保証する。

1. `HidReportSource` は、hidapi の `HidDevice` を内に持ち、**外には「読む」と「閉じる」だけを出す**。送る関数、機能レポートを読む関数、生の `HidDevice` を返す関数を作らない。
2. M2 のコードは、命令を組み立てる関数を持たない（`commands.md` の 6 章の形は M3 で作る）。
3. **検査（テスト）**: M2 のクレートの `src/` の下のすべての `.rs` ファイルを読んで、次の 2 つを確かめるテストを置く。
   - `hidapi` の名前が出てくるファイルは、`HidReportSource` のファイル（13 章の `src/source_windows.rs`）の 1 つだけ。
   - どのファイルにも、`send_feature_report`、`get_feature_report`、`send_output_report`、`HidD_SetFeature`、`HidD_GetFeature`、`HidD_SetOutputReport`、`WriteFile` がない。`HidReportSource` のファイルには、さらに `.write(`（hidapi の `HidDevice` の送信）もない（記録のファイルへの書き込みは、ほかのファイルで行うので、この語はそのファイルだけで調べる）。
   
   例外を書く場所は作らない（M3 で送る部分は、別のクレートか別のモジュールに作り、そのときにこの検査の範囲を設計で決め直す）。
4. レビュー担当は、M2 の PR で、依存に送る部分を持つクレートが増えていないか（hidapi のほかに HID を扱うクレートがないか）を見る。

## 5. 型

名前は案。実装担当は、意味を変えない範囲で直してよい（直したら PR の説明に書く）。

```rust
/// One input report as received, before any decoding.
pub struct RawReport {
    pub mono_us: u64,      // microseconds since the start of the recording (monotonic)
    pub wall_ms: i64,      // Unix time in milliseconds (UTC) when received
    pub bytes: Vec<u8>,    // as returned by hidapi (report id 0 already removed on Windows)
}

/// One item of the live input stream. Only the reader thread creates these, reading the clock
/// once per item, so items reach the main thread in non-decreasing `mono_us` order (3, 10).
pub enum SourceItem {
    Report(RawReport),
    Idle { mono_us: u64, wall_ms: i64 },                       // read timed out (100 ms) with nothing
    ReadError { mono_us: u64, wall_ms: i64, os_code: i32 },
    Removed { mono_us: u64, wall_ms: i64 },                    // device gone; the reader thread ends
}

/// Opaque per-recording tracker label: "A", "B", ... in order of first appearance.
pub struct TrackerLabel(pub char);

pub enum DongleEvent {
    Pair { tracker: TrackerLabel, unpaired: bool },
    Text { tracker: TrackerLabel, message: TrackerMessage },
    Pose { tracker: TrackerLabel, sample: PoseSample },
    RfStatus { slots: [SlotStatus; 5] },
    OtherTrackerData { tracker: TrackerLabel, kind: u16, len: usize },
    Unknown { report_type: u8, len: usize },
    Malformed { report_type: u8, reason: &'static str },
}

pub enum TrackerMessage {
    DeviceInfo { field: DeviceInfoField, value: Redactable },   // NADS, NASS, NAV1, NARI, ...
    MapStatus { a: i32, b: i32 },                               // MS<a>,<b>
    LambdaStatus(Vec<i32>),                                     // LS<a>,<b>,<c>
    SlamStatus { key: u8, value: i64 },                         // P61:<key>,<value>
    SlamError { code: u32 },                                    // P82:0:<code>
    WifiHostInfo,                                               // WH... (values are never kept)
    WifiInfoRequest,                                            // WS
    WifiConnected { ok: bool },                                 // WC<n>
    PowerOff, Reset,                                            // APF, APR
    Unknown { prefix: String },                                 // first 2 chars only
}

pub struct PoseSample {
    pub slot: u8,
    pub buttons_raw: u8,             // not interpreted in M2 (protocol 4.4 note)
    pub position_m: [f32; 3],
    pub rotation_wxyz: [f32; 4],     // reordered from the wire order w, z, y, x
    pub acceleration: [f32; 3],
    pub angular_velocity: [f32; 4],
    pub tracking_status: i8,         // -1..=4, protocol 4.4
    pub extra: Vec<u8>,              // bytes after offset 37, kept raw
}

pub struct SlotStatus { pub status: u8, pub timeout_ms: u16, pub flags: u8 }

/// One line of a recording, as read back (8.2).
pub enum RecordLine {
    Header(Header),
    In(RawReport),
    Tick { mono_us: u64, wall_ms: i64 },              // written every second, even when nothing arrives
    Note { mono_us: u64, wall_ms: i64, word: NoteWord },   // fixed words only (8.2); OS codes as numbers
    Omitted(OmittedReport),                           // only in an anonymized export (8.4)
    Footer(Footer),
}

/// A report left out of an anonymized export. Never carries the report's bytes.
pub struct OmittedReport {
    pub mono_us: u64,
    pub wall_ms: i64,
    pub report_type: u8,
    pub len: usize,
    pub tracker: Option<TrackerLabel>,                // when the 0x28 header could be read
    pub reason: OmitReason,                           // NotAllowed, CheckFailed, TextForm, Malformed
}

/// A change of `Link`, stamped with the time it happened (not the time it was noticed).
pub struct Transition { pub at_us: u64, pub tracker: TrackerLabel, pub from: Link, pub to: Link }

impl TrackerTable {
    /// Applies one decoded report. Calls `advance(mono_us)` first.
    /// Callers pass reports and `advance` times in one stream order (6.4); a time smaller
    /// than the last one is counted in `time_reversed` and treated as the last time.
    pub fn apply(&mut self, mono_us: u64, event: &DongleEvent) -> Vec<Transition>;
    /// Moves time forward with no report: marks trackers silent whose deadline has passed
    /// and recomputes the pose rate over the last second.
    pub fn advance(&mut self, now_us: u64) -> Vec<Transition>;
    /// How many times a smaller time than the last one was passed (should stay 0).
    pub fn time_reversed(&self) -> u64;
}

/// What the console shows for one tracker (DG-1).
pub struct TrackerView {
    pub label: TrackerLabel,
    pub link: Link,                          // NotSeen, Receiving, Silent, Unpaired
    pub tracking_status: Option<i8>,
    pub map_state: Option<i64>,
    pub is_host: Option<bool>,
    pub role_id: Option<String>,
    pub wifi_connected_to_host: Option<bool>,
    pub firmware: Option<String>,
    pub last_pose: Option<PoseSample>,
    pub pose_rate_hz: f32,
    pub last_error: Option<u32>,
    pub last_seen_us: u64,
}
```

- `Redactable` は、個人の情報を含む値（シリアル番号など。`m2-dongle-protocol.md` の 4.5 の表）を、表示と書き出しで伏せるための包み。`Debug` と `Display` では中身を出さない。
- `TrackerLabel` は、トラッカーの宛先（無線のアドレスにあたる）を外に出さないための名前。宛先と名前の対応は、記録 1 本の中だけで持ち、表示しない。

## 6. 解読の決まり

### 6.1 共通

- **止まらない。** どんなバイト列でもパニックしない。長さが足りない、知らない種類、文字列が ASCII でない、は `Malformed` か `Unknown` にして数えるだけ。
- 先頭が `0x00` で、2 バイト目が知っている種類なら、1 バイトずらして読む（hidapi の版や OS で、レポート番号の 0 が残った場合のため。U-3）。
- 位置と形は `m2-dongle-protocol.md` の 4 章のとおり。確かさが「未確認」の欄は、値を解釈せずに生のまま残す（電池とボタンのバイト、姿勢の残りのバイト、`0x29`）。
- 16 ビットの浮動小数点は、自前の関数で 32 ビットに直す（依存を増やさない。0、非正規化数、無限大、NaN をテストする）。

### 6.2 トラッカーの名前

- トラッカーは、宛先の 6 バイトのうちの識別子の 5 バイトで見分ける。**識別子の位置は、報告の種類ごとに違う**（`m2-dongle-protocol.md` の 4.7: `0x28` は 4〜8、`0x18` は 7〜11）。位置は、コードの 1 か所（`src/decode/addr.rs` の表）にだけ書き、解読と書き出し（8.4）の両方がそれを使う。
- 種類をまたいで、同じ 5 バイトなら同じ名前にする。初めて見た順に A、B、C… と名付ける（記録 1 本の中で固定）。
- `0x18` の識別子が、それまでの `0x28` のどれとも合わないときは、新しい名前にする。`summary` に「ペアリングの知らせだけに現れた名前」の数を出す（位置の読み違いか、宛先の形の違いの印。U-4）。
- `0x1D`・`0x1E`・`0x29` と知らない種類からは、宛先を読まない（位置が分からないため）。
- S2 が使う「宛先の 2 バイト目の下位 4 ビット = 枠の番号」は、表示に「枠 n（推定）」として添えるだけにする（U-4）。

### 6.3 文字列の知らせ

- 先頭に数字が 1 文字あれば、枠の番号として外してから、先頭の文字で種類を決める（`m2-dongle-protocol.md` の 4.5 の表。U-6）。
- 個人の情報を含む種類（`NADS`、`NASS`、`NASI`、`NAPI`、`NADN`、`NADI`、`C…`、`c…`、`WH…`、`Ws…`、`Wp…`、`Wt…`）は、値を `Redactable` に包む。個人の情報を含むかが未確認の `NANA` も、含むとみなす。`WH` は値を持たない（`WifiHostInfo`。ホストの印として使う）。
- 表にないものは `Unknown { prefix }`（先頭 2 文字だけ）。

### 6.4 トラッカーの状態表（`TrackerTable`）

出来事と時刻を受けて、トラッカーごとの `TrackerView` を進める。時刻は、呼ぶ側が渡したもの（`apply` の `mono_us` と `advance` の `now_us`）だけを使う。

| 出来事 | 変わるもの |
|---|---|
| `Pair { unpaired: false }` | `link` = 受信中（データが来るまでは「つながった」） |
| `Pair { unpaired: true }`、`PowerOff`、`Reset` | `link` = 外れた |
| `Pose` | `last_pose`、`tracking_status`、`pose_rate_hz`（直前 1 秒の件数）、`last_seen_us`。`link` = 受信中 |
| `Text` のどれか、`OtherTrackerData` | `last_seen_us`。`link` = 受信中 |
| `MapStatus { b }`、`SlamStatus { key: 5 }` | `map_state`（どちらが VIVE Hub の `map state` にあたるかは未確認なので、両方を別に持ち、表示では出所を添える） |
| `SlamStatus { key: 6 }` | 追跡の状態の写し（姿勢の中の状態と食い違えば、両方を記録する） |
| `WifiHostInfo` | `is_host` = はい |
| `WifiInfoRequest`、`WifiConnected` | `is_host` = いいえ。`wifi_connected_to_host` |
| `DeviceInfo { NARI }`、`{ NAV1 }` | `role_id`、`firmware` |
| `SlamError { code }` | `last_error` |
| 渡された時刻が、その台の最後の受信から **2.0 秒**（仮の値。12 章）を過ぎた（`apply` でも `advance` でも） | `link` = 途切れた。移り変わりの時刻は、気づいた時刻ではなく、期限の時刻（最後の受信 + 2.0 秒）にする |

**報告がまったく来ないとき**（全台が止まった、ドングルが黙った）も、途切れの判定を進める。

- `advance(now_us)` は、報告がなくても、期限を過ぎた台を「途切れた」にし、姿勢の毎秒の件数を直前 1 秒の窓で数え直す（0 に下がる）。
- `apply` は、出来事を当てる前に `advance(mono_us)` を呼ぶ。同じ台の報告のあいだに 2.0 秒より長い空きがあれば、先に「途切れた」、そのあとに「受信中」になる。
- **順序の決まり**: `advance(t)` を呼ぶのは、時刻が t までの報告をすべて当てたあとだけ。呼ぶ側（実機の主のループと、再生）は、報告と時刻を 1 本の流れの順に渡す（3 章）。実機では、読み取りのスレッドが時計を読んだ順に流れを作り、主のスレッドは流れの時刻だけを使うので、主のスレッドの処理が遅れても、この決まりは崩れない（10 章）。
  - 例: A の受信が 10.000 秒と 11.990 秒にあり、11.990 秒の報告がまだ主のスレッドに渡っていないあいだに、時計は 12.100 秒になった。流れは「11.990 秒の報告」→「12.090 秒ごろの `Idle`」の順なので、主のスレッドは先に 11.990 秒の報告を当て、A の期限は 13.990 秒に延びる。A は途切れない。記録にも同じ順で行が並ぶので、再生でも途切れない。主のスレッドが自分で時計を読んで `advance(12.100)` を呼ぶ作りにすると、実機だけ A が 12.000 秒で一度途切れ、再生と食い違う（この作りにしない）。
- 移り変わりの時刻を期限の時刻にするので、この決まりを守っていれば、`advance` を呼ぶ間隔（実機は `Idle` ごと、およそ 100 ms。再生は記録の行ごと）が違っても、実機と再生で同じ移り変わりと時刻になる。
- 渡された時刻が前より小さいときは、前の時刻のまま扱い、`time_reversed` に数える（順序の決まりが破られた印。起きない作りにしてあり、試験で 0 を確かめる。数が 0 でなければ、締めの行と `summary` に出す）。
- 再生で、締めの行も `tick` の行もなく記録が終わったときは、最後の行の時刻までしか進めない（その先は分からない）。`tick` を毎秒書くので（8.2）、分からないのは最後の 1 秒まで。

M1 の状態（PRD の W-2: 未接続、接続、Wi-Fi 待ち、地図同期中、位置取り直し中、追跡中）への対応は、M3 で入力を差し替えるとき（R-9）に決める。見込みは `m2-dongle-protocol.md` の 8 章の表。

## 7. VIVE Hub のサーバーの扱い（DG-3）

### 7.1 見張り（`ServerGuard`）

- 判定は純粋な関数にする: プロセスの名前の一覧を受けて、「動いている」「動いていない」を返す。名前は大文字と小文字を区別せずに `ViveTrackerServer.exe` と比べる（ログのファイルの名前 `ViveTrackerServer_YYYYMMDD.txt` と、S3 の記録による）。名前の一覧は設定で足せるようにする（VIVE Hub の版で名前が変わったとき）。
- プロセスの一覧は `sysinfo` で取る（10 章）。
- `record` は、始める前に確かめ、動いていれば**ドングルを開かずに**断る（終了コード 2）。記録の途中も **2 秒ごと**（流れの時刻で数える。流れが 1 秒より長く届かないときも確かめる。10 章）に確かめ、現れたら、読むのをやめてドングルを閉じ、記録に `note` を書いて終わる（終了コード 2）。

### 7.2 断るときの案内（意味）

> VIVE Hub のトラッカー用サーバー（ViveTrackerServer.exe）が動いています。ドングルは同時に 1 つのプログラムでしか扱えない前提なので、読み取りを始めません。VIVE Hub を終了し、タスク マネージャーで ViveTrackerServer.exe がないことを確かめてから、もう一度実行してください。

### 7.3 前提の確認（`probe-sharing`）

DG-3 の「前提が正しいかも確かめる」のための試し。**ふだんの使い方ではない。**

- 開始時と、基準の測定後にドングルを開く直前に、サーバーが動いているか確認する。基準の測定後に停止していたら、ドングルを開かずに中止する。記録には中止の覚え書き `probe_server_stopped` と終了理由 `server_not_running` を残し、試しの窓の開始・終了時刻は書かない。
- 2 つの窓で動く。どちらの窓でも、何も送らない。
  1. **基準の窓**（N 秒。既定 30）: ドングルを開かずに待つ。VIVE Hub のログに、ふだんの途切れと切断がどれだけあるかを、あとでくらべるため。
  2. **試しの窓**（N 秒）: ドングルを共有で開こうとし、結果（開けた／開けなかった、と OS の誤りの番号）を出す。開けたら N 秒だけ読み、種類ごとの件数を出し、`record` と同じ形で記録する。終わったら閉じる。
- 画面と記録（覚え書き `window_base_start` などの決まった語。8.2）に、測定を行った窓と共有で開けなかった試しの窓の開始・終了時刻（UTC と、`--utc-offset` があればその時差の時刻。秒まで）を出す。VIVE Hub のログから対応する時間帯を切り出せる。
- 基準の窓の 2 つの覚え書きと `window_probe_start` は、読み取りスレッドを始める前に、同じ時計（`Clock`）で時刻を付ける。共有で開けた場合は、読み取り開始時刻の N 秒後を期限とする。読み取りスレッドは、期限までの報告を受信順に渡し、期限後の報告は渡さない。`window_probe_end` には、期限で終了した場合は期限の時刻を、読み取りエラーや取り外しで早く終了した場合はその時刻を書く。
- **分かることは 3 つの別の観測に分ける**。どれか 1 つから、ほかは言えない。

| 観測 | 何で見るか | 分かること |
|---|---|---|
| B-1 開けたか | 道具の出力（開けた／開けなかった、誤りの番号） | U-2 の前半。開けなければ、読むことについても DG-3 の前提は正しい |
| B-2 こちらで読めたか | 道具の出力と記録（種類ごとの件数） | U-2。開けても読めない（件数が 0）こともありうる |
| B-3 VIVE Hub 側の取りこぼしが増えないか | VIVE Hub のログで、基準の窓と試しの窓をくらべる。くらべる行（姿勢の状態の変化、`OET LOST TRACKING!`、`Remove tracker` と `Add new tracker` など）は、#7 の書式のメモに合わせて PM が決める。あわせて、試しのあとに VIVE Hub の画面で 3 台が追跡中のままか | U-2 の後半（開いたハンドルごとに写しが届くか）、U-17 |

- 30 秒ずつのくらべでは、差が小さいと言い切れない。差がなかったときは「この長さでは増えなかった」と書く。
- B-1 と B-2 が「はい」でも、B-3 を確かめられないとき（ログのくらべ方が決まっていない、など）は、「VIVE Hub と並べて読む」使い方の判断材料にしない。
- 開けて、B-3 で VIVE Hub に影響がないと分かった場合に、VIVE Hub と並べて読む使い方を許すかは、DG-3 の変更になるので、PM が決める（PR の「未解決」）。

## 8. 記録の形（DG-4）

### 8.1 決まり

- **受け取った報告を、解読せずにそのまま**（16 進の文字列）、時刻つきで残す。解読の誤りが分かっても、記録を取り直さずに解読し直せる。
- 形は JSON Lines（UTF-8、1 行に 1 つの JSON）。1 行目が見出し、あとは報告、時刻の行（`tick`）、覚え書き、最後に締め。
- 読む側は、知らない `type` の行と、知らない欄を無視する。壊れた行（JSON として読めない、途中で切れた最後の行）は飛ばして数える。締めの行がなくても読める（Ctrl+C で止めたとき）。
- 形を互換でなく変えるときは、`version` を上げる。現在はバージョン 2 で書き、バージョン 1 と 2 を読める。読む側は、知らない `version` を断る。

### 8.2 行の種類

値は例（`usage_page` などは、試し A で分かるまで決まっていない）。

```jsonl
{"type":"header","format":"openut.dongle-record","version":2,"tool":"openut-dongle 0.1.0","mode":"record","started_wall_ms":1764302512123,"device":{"vid":"0bb4","pid":"0350","interface":0,"usage_page":"ff00","usage":"0001"},"server_running_at_start":false,"anonymized":false}
{"type":"in","mono_us":1520,"wall_ms":1764302512125,"hex":"28a1003a…"}
{"type":"tick","mono_us":1000412,"wall_ms":1764302513123}
{"type":"note","mono_us":60000000,"wall_ms":1764302572123,"text":"server_appeared"}
{"type":"footer","mono_us":120000000,"reports":29280,"malformed":0,"reason":"duration"}
```

| 欄 | 意味 |
|---|---|
| `mode` | `record` か `probe-sharing` |
| `device` | 機器の種類だけ。**シリアル番号と、機器の通り道（パス）は入れない**（パスに機器ごとの番号が入るため） |
| `mono_us` | 記録の始まりからの単調な時刻（マイクロ秒）。報告の行は、読み取りのスレッドが受け取ったときに付けた時刻（`SourceItem`）。**行は流れの順に書くので、ファイルの中で `mono_us` は減らない**。再生は、ファイルの行の順に行う（並べ替えない。減っている行があれば、壊れた記録として数え、前の時刻のまま扱う） |
| `wall_ms` | 受け取ったときの UTC の Unix 時刻（ミリ秒）。読み取りのスレッドが `mono_us` と同時に読む。VIVE Hub のログとの突き合わせに使う |
| `hex` | 受け取った報告（hidapi が返したまま） |
| `tick` | 時刻だけの行。主のスレッドが、流れの時刻が前の `tick` から 1 秒以上進むたびに、**その流れの項目（報告か `Idle`）の時刻**で書く。報告が来ないあいだも `Idle` で流れの時刻は進むので、毎秒書かれる。再生で、報告が来ないあいだも時刻を進めるため（6.4）。主のスレッドが自分で時計を読んで書くことはしない（順序の決まり。6.4） |
| 主のスレッドが書く行の時刻 | `note`（`server_appeared` など）と締めは、主のスレッドがそれまでに当てた流れの最後の項目の時刻（`mono_us` と `wall_ms`）で書く。`read_error`・`device_removed` は、流れの `ReadError`・`Removed` の時刻で書く。`probe-sharing` の窓の覚え書きは 7.3 |
| `time_reversed` | 締めの行の欄。`TrackerTable` に前より小さい時刻が渡された回数（6.4）。0 なら書かない |
| `note` の `text` | 決まった語だけ: `server_appeared`、`read_error:<OS の番号>`、`device_removed`、`duration`、`window_base_start`・`window_base_end`・`window_probe_start`・`window_probe_end`（`probe-sharing` の窓。7.3）、`open_failed:<OS の番号>`、`probe_server_stopped`（基準の測定後に試しを中止。7.3）。語を足すときは、この表を直す（8.4 の書き出しは、この表にある語だけを写す） |
| `omitted` | 共有用の書き出し（8.4）だけにある行。書き出しに入れなかった報告の、時刻、種類、長さ、名前（分かれば）、理由。**報告の中身は書かない**。再生では、名前があればその台の受信（`last_seen_us`）として、なければ時刻だけとして扱う（元の記録と書き出しで、つながりの移り変わりが同じになるため） |
| `estimated_lost` | 旧版の記録にある参考値。報告番号がどの範囲で連続するかは未確認のため（U-11）、新しい記録では省略する。読み込み時は省略を「不明」として扱う |

### 8.3 置き場所と大きさ

- 既定の置き場所は `%LOCALAPPDATA%\OpenUT\dongle\`、名前は `YYYYMMDD-HHMMSS-<mode>.jsonl`（UTC）。`--out` で変えられる。利用者の名前をコードに書かない（W-9 と同じ考え）。
- 毎秒約 244 件（U-16）なら、1 行約 190 バイトで、毎分約 2.8 MB、10 分で約 28 MB（`tick` の行は毎秒 60 バイトほどで、数に入らない）。`record` の長さは既定 10 分、最大 60 分。
- **記録そのものには、トラッカーの宛先、シリアル番号、Wi-Fi の名前とパスワード（`WH`）が入りうる。** 記録はオーナーの PC の中だけに置き、レポジトリと Issue には、8.4 の書き出しを通したものだけを入れる。`record` の終わりに、そのことを一行で知らせる。

### 8.4 共有用の書き出し（`export --anonymize`）

**共有用の書き出しは、元の報告を書き換えて作らない。報告を解読し、安全と判断できた欄だけから作り直す**（許した欄の一覧。8.4.2）。未知・不正・未解読の部分は、書き出しに入らない。元の報告は、オーナーの PC の記録（8.3）だけに残る。置き換えの値は、L-1（#8 の匿名化の道具）と同じ置き換えの表を使う。

決まった場所だけを置き換える形にしない理由: 未知の報告や、姿勢の未解読の部分に、別の形の識別子や Wi-Fi の値が入っていたとき、確かめる側もその値を知らないので、そのまま残る。作り直す形なら、知らない部分は最初から書き出しに入らない。

#### 8.4.1 作り直しの決まり

1. 記録の行を順に読み、報告を 6 章と同じ解読にかける。トラッカーの名前も、同じ決まり（6.2。宛先の位置は `m2-dongle-protocol.md` の 4.7）で付ける。
2. 報告ごとに、8.4.2 の表の行を選ぶ。表の「検査」に通ったら、**元と同じ長さの、すべて 0 のバイト列**を用意し、「残す欄」だけを書き込む。それ以外の位置は 0 のまま。
3. 表で「落とす」報告と、検査に通らない報告は、作り直さずに `omitted` の行（8.2）にする。中身は書かない。`0x28` の頭が読めれば、名前を添える。
4. 宛先は、名前ごとの合成の宛先（6 バイト）`00 A<n> 00 00 00 <k>` にし、報告の種類ごとの位置（4.7）に書く。`<n>` は、元の宛先の 2 バイト目の下位 4 ビット（枠の番号の推定。4 ビットでは機器を特定できないので、U-4 の手がかりとして残す）、`<k>` は名前の番号（A = 1、B = 2…）。`0x28` と `0x18` で、同じ名前には同じ合成の宛先が入る。
5. 個人の情報を含む文字列の知らせ（6.3 の一覧）は、種類の文字（と先頭の枠の番号の数字）だけを残し、値を**同じ長さ**の置き換えにする（長さの欄を壊さないため）。置き換えは #8 の置き換えの表を使い（ファイルをまたいで同じ値は同じ置き換え）、表がなければ、値ごとに決まった文字列（`X` で埋めて末尾に番号）にする。
6. 見出しは、`format`・`version`・`tool`・`mode`・`started_wall_ms`・`server_running_at_start` と `device` の決まった欄（8.2）だけを写し、`anonymized` を `true` にする。`tick` は写す。覚え書き（`note`）は、8.2 の表にある決まった語に一致するものだけを写す（一致しないものは落として数える）。締めは件数を写し、`omitted` の件数を、報告の種類と理由ごとに足す。
7. **残っていないかの確認**（作り直しの誤りへの二重の備え）: 元の記録から、トラッカーの宛先（6 バイト）と識別子（5 バイト。4.7 の種類ごとの位置で取ったもの）と、個人の情報を含む文字列の値（`WH` は名前とパスワードに分けたもの。4 文字以上のもの）を集める。書き出したファイルの中（作り直した報告のバイト、`hex` の文字列を大文字と小文字を区別せずに、文字列の知らせ）に、ひとつも現れないことを確かめる。現れたら、ファイルを書かずに終わる（終了コード 5）。この確認は、**知っている値しか探せない**。知らない値を落とすのは 2・3 の作り直しの役目で、確認の役目ではない。
8. 終わりに、書き出しに入れなかった件数を、報告の種類と理由ごとにコンソールに出す（例: 「`0x29` を 15 件、形に合わない文字列の知らせを 3 件、書き出しに入れませんでした」）。

#### 8.4.2 許した欄の一覧

位置と意味は `m2-dongle-protocol.md` の 4 章。**この表にない報告と欄は、書き出しに入らない。**

| 報告 | 残す欄 | 検査（通らなければ `omitted`） | 0 にする部分 | 安全と判断した理由 |
|---|---|---|---|---|
| `0x28` の頭（下の `0x28` のすべてに共通） | 種類、通し番号、合成の宛先、中身の種類、中身の長さ | 中身の長さが報告に収まる | 元の宛先の 6 バイト（合成の宛先に替える） | 通し番号は数えるだけの数。宛先は合成のものに替える |
| `0x28` 姿勢（中身の種類 `0x0110`） | トラッカーの番号、ボタン・電池の 1 バイト、位置、向き、加速度、角速度の 1〜3 つ目、追跡の状態 | 中身の長さが 37・39・50 のどれか。トラッカーの番号が 0〜4。追跡の状態が -1〜4。浮動小数点がすべて有限。位置の大きさが 100 m 以下。向き（四元数）の大きさが 0.9〜1.1（ただし、位置と向きがすべて 0 のものは通す。追跡していないときの値の見込みで、何も運ばない） | 角速度の 4 つ目（意味が未確認）、中身の 37 バイト目から後（不明）、中身のあとの残り | 意味の分かった数の欄だけ。位置の読み違いがあれば、向きの大きさの検査で落ちる。ボタン・電池の 1 バイトは、S2 と S3 の `tracker_report.py` が同じ位置（中身の 1 バイト目）を読み、意味だけが食い違う（`m2-dongle-protocol.md` の 4.4 の注）。1 バイトには識別子（5 バイト）も文字列も入らない。U-5 の突き合わせ（VIVE Hub の画面の電池の割合）に要るので残す |
| `0x28` 文字列の知らせ（中身の種類 `0x0101`）で、個人の情報を含まない種類 | 文字列のまま | 文字列の全体が、種類ごとの形（表の下の注）に一致する | 中身のあとの残り | 形が決まっていて、数と決まった文字しか入らない |
| `0x28` 文字列の知らせで、個人の情報を含む種類（6.3 の一覧） | 種類の文字と、同じ長さの置き換え | 種類の文字のあとが、印字できる ASCII | 元の値、中身のあとの残り | 値を持ち出さない |
| `0x18` ペアリングの知らせ（位置は `m2-dongle-protocol.md` の 4.6） | 種類（0）、長さ（1）、外れたか（5）、合成の宛先（6〜11） | 外れたかが 0 か 1。長さが 12 以上で、報告に収まる | 2〜4（不明の 3 バイト。4 は枠の番号の可能性があるが、意味が未確認なので落とす）、元の宛先の 6 バイト、12 から後の残り | 意味の分かった 1 バイトの欄 2 つと、合成の宛先だけ。**位置が実機で違っていても**、残すのはこの 3 つで、ほかは 0 になるので、元の宛先は書き出しに入らない（読み違いは、名前が合わないことで分かる。6.2、U-4） |
| **落とす**: `0x28` のそのほかの中身の種類、`0x1D`・`0x1E`、`0x29`、知らない種類、`Malformed` | — | — | 報告の全体（`omitted` の行にする） | 中身が解読できていない（`0x1E` は、枠の記録が始まる位置を、経路の違う 2 つの解析で数えただけで、確かめていない。`m2-dongle-protocol.md` の 4.6）。確かめたあと、設計の追補でこの表に足す |

文字列の形（個人の情報を含まない種類。先頭に枠の番号の数字が 1 つ付いてよい）: `NAV1` は数字とピリオド（32 文字まで）、`NARI` は整数 1 つ、`NAGN` は整数 3 つ、`MS` は整数 2 つ、`LS` は整数 1〜8 個、`P61:` は整数 2 つ、`P82:0:` は整数 1 つ、`WC` は整数 1 つ、`WS`・`APF`・`APR`・`NAZZ` はその文字だけ。整数は符号つきの 10 けたまで、区切りはコンマ。形に合わないもの（`LP…`、`DEC…`、表にない種類、印字できる ASCII でないもの、途中で切れたもの）は `omitted`（理由 `text_form`）。

- `omitted` の理由: `not_allowed`（表で落とす報告）、`check_failed`（検査に通らない）、`text_form`（文字列の形に合わない）、`malformed`（解読できない）。
- 姿勢（位置と向き）は残す。体の動きの記録にあたるので、オーナー以外の利用者から集めるときは L-4 の決まり（送る前に示す）に従う。
- 落とした部分（`0x29`、`0x1E`、姿勢の残りのバイト、形に合わない文字列の知らせ）の中身は、レポジトリに入らない。中身を調べる必要が出たとき（U-5、U-7、U-8）の手順は、試し C の結果を見て、設計の追補で決める（表に足すのは、そのときに意味と安全を確かめた欄だけ）。

## 9. 道具の使い方

### 9.1 命令（サブコマンド）

| 命令 | 何をするか | 動く OS | ドングルに触るか |
|---|---|---|---|
| `openut-dongle list` | VID `0x0BB4` の HID 機器を並べる（VID、PID、インターフェース、用途、製品名）。シリアル番号とパスは出さない | Windows | 一覧を取るだけ。hidapi は、読み書きの権限なしで開いて属性を読み、製品名などの文字列を USB の標準の要求で読む（S5）。機器の命令は送らない |
| `openut-dongle check` | サーバーが動いているか、ドングルがあるかを出す。読み取りを始められるかの答え | Windows | 開かない |
| `openut-dongle record [--minutes N] [--out PATH]` | 読んで、記録しながら、1 秒ごとに状態を出す | Windows | 開いて読む（送らない） |
| `openut-dongle probe-sharing [--seconds N] [--utc-offset +9]` | 前提の確認（7.3）。基準の窓（開かない）と試しの窓（開いて読む）を N 秒ずつ。N は既定 30、最大 60 | Windows | 試しの窓だけ、開いて読む（送らない） |
| `openut-dongle decode RECORD` | 記録を解読して、出来事を 1 行ずつ JSON で出す（M3 の再生テストの期待値づくりにも使う） | どれでも | 触らない |
| `openut-dongle summary RECORD [--utc-offset +9] [--out FILE]` | トラッカーごとに、見えた時間、受信中の区間（始まりと終わりの時刻、区間の中の姿勢と文字列の知らせの件数）、追跡の状態ごとの件数と切り替わりの時刻、地図の状態、知らない報告の件数、取りこぼしの記録値（旧版の参考値。欄がなければ「不明」）を出す。ほかに、ペアリングの知らせだけに現れた名前の数（6.2）。VIVE Hub のログとの突き合わせと、試し C の C-1・C-2 の読み分け（11 章）に使う。書き出し（8.4）にも使える | どれでも | 触らない |
| `openut-dongle export --anonymize RECORD OUT` | 8.4 の共有用の書き出し（許した欄だけで作り直す） | どれでも | 触らない |

集計をファイルに残すときは `--out FILE` を指定します。コンソールと同じ内容を BOM なしの UTF-8 で保存します。既存のファイルは上書きしません。`--out` を省くとコンソールに表示します。

Windows PowerShell 5.1 では `--out` を使うと、パイプを通すときの文字化けを避けられます。ファイル名に空白がある場合は引用符で囲みます。

```powershell
openut-dongle summary "capture sample.jsonl" --utc-offset +9 --out "capture summary.txt"
```

終了コード: 0 正常、1 使い方の誤り、または `probe-sharing` に必要な VIVE Hub のサーバーが停止、2 VIVE Hub のサーバーが動いているため `record` を終了、3 ドングルがない・開けない・外れた、4 記録や集計結果をファイルに書けない、5 匿名化の確認で元の値が残った。

### 9.2 何をいつ見せるか（コンソール）

- **始めに**: 見つけたドングルの種類、記録の置き場所、止め方（Ctrl+C か、時間が来たら止まる）。
- **1 秒ごとに**: 経過の秒数、ドングル全体の毎秒の受信件数、トラッカーごとに 1 行（名前、つながり、追跡の状態の番号と意味、位置 x・y・z（m、小数 2 けた）、姿勢の毎秒の件数、地図の状態、ホストかどうか（分からなければ「不明」）、電池とボタンのバイト（16 進のまま））。
- **出来事のたびに**: つながった、外れた、途切れた、SLAM の誤り（番号と意味）を 1 行。
- **終わりに**: 止まった理由、件数、記録の場所、「記録には個人の情報が入りうるので、人に渡すときは `export --anonymize` を使う」の一行。
- **出さないもの**: トラッカーの宛先、シリアル番号、Wi-Fi の名前とパスワード、機器のパス。

文言は日本語（追跡の状態の意味は `m2-dongle-protocol.md` の 4.4 の表）。行の見た目は実装担当が決めてよい。**UI/UX デザイン担当への依頼は要らない**（窓を持たないため）。

時刻は、記録には UTC で残す。画面と `summary` の時刻の表し方（日本時間で出すか）は、M1 の設計（#5）が選ぶ時刻の扱いに合わせる。それまでは、経過の秒数と UTC を出し、`summary` は `--utc-offset` で時差を受ける（VIVE Hub のログの行には `+9` のような時差が書かれている）。

## 10. Rust での作り方

| 用途 | 候補 | 版・ライセンス・保守 | 使い方 |
|---|---|---|---|
| HID | **hidapi**（https://crates.io/crates/hidapi 、https://github.com/ruabmbua/hidapi-rs ） | 2.6.7（2026-08-27）。MIT。2025-12 から 2026-08 に 4 回の版がある。ダウンロードは約 1,100 万 | Windows だけの依存にする: `[target.'cfg(windows)'.dependencies] hidapi = { version = "2.6.7", default-features = false, features = ["windows-native"] }`。`windows-native` は Rust だけで書かれた Windows の実装（`windows-sys` を使う）で、C のコンパイラーが要らない。開くときは読み書きの権限と共有（S5）。**このドングルで `windows-native` が C の実装と同じに動くかは未確認**（試し A）。駄目なら C の実装（既定の機能）に替える。そのときの C の hidapi のライセンスは未確認なので、替える PR で確かめる |
| プロセスの一覧 | **sysinfo**（https://crates.io/crates/sysinfo ） | 0.40.4（2026-11-16）。MIT。必要な Rust 1.95（固定の 1.100 で足りる）。頻繁に版が出ている | `default-features = false, features = ["system"]`。名前の一覧を取るだけ。直接 Win32 の API（Toolhelp）を呼ぶと `unsafe` が要り、クレートの `unsafe_code = "deny"`（PR #2 の `Cargo.toml`）に合わないので、こちらにした |
| 記録の JSON | **serde**、**serde_json** | serde 1.0.229（2026-07-18）、serde_json 1.0.154（2026-10-11）。どちらも MIT OR Apache-2.0 | 見出し・報告・覚え書き・締めの行。M1 も同じものを使う見込み（#5 の設計に合わせる） |
| 16 ビットの浮動小数点、16 進 | 自前 | — | 数十行。依存を増やさない |
| Ctrl+C | 使わない | — | 止めると締めの行がないまま終わる。記録は 1 秒ごとに書き出すので、失うのは最後の 1 秒まで（8.1 で読めるようにしてある） |

- 依存の版は、追加する PR で crates.io の情報を確かめ直す（AGENTS.md）。上の数字は 2026-11-28 に crates.io の API で確かめた。
- Windows の部分（`HidReportSource`、`ProcessList`、`list`・`check`・`record`・`probe-sharing`）は `#[cfg(windows)]` にする。Linux の CI では、解読、状態表、記録、書き出し、要約、`decode`・`summary`・`export` のテストが動く。Windows の CI では、すべてが組み立てられ、clippy が通る。
- **読み取りのスレッド**（`reader_loop`）は、`read_timeout`（100 ms）で回す。報告を受け取ったら、すぐ時計（`Clock`。記録の始まりからの経過は `std::time::Instant`、UTC は `SystemTime`）を読んで `SourceItem::Report` にし、時間切れなら時計を読んで `SourceItem::Idle` にし、読み取りの誤りと機器が外れたときも時計を読んで `ReadError`・`Removed` にして、チャンネル（`std::sync::mpsc`。1 つの送り手から送った順に届く）で主のスレッドへ渡す。**時計を読むのは、このスレッドだけ**（`probe-sharing` の読み取りを始める前の覚え書きを除く。7.3）。1 つのスレッドが読んだ順に流れを作るので、流れの `mono_us` は減らない。入力のためは hidapi が 64 件にする（`m2-dongle-protocol.md` の 7 章）。毎秒約 244 件なら、読み取りが 0.25 秒止まるとあふれる見込みなので、読み取りのスレッドでは書き込みも解読もしない。
- **主のスレッド**（`main_loop`）は、流れを届いた順に 1 つずつ処理する: 記録の行を書く、解読して `TrackerTable::apply` を呼ぶ（`Idle` なら `advance` だけ）、流れの時刻が前の `tick` から 1 秒以上進んだら `tick` の行を書いて表示を出す、流れの時刻で 2 秒ごとにサーバーを確かめる（7.1）、流れの時刻で `record` の長さを測る。`probe-sharing` では期限後の項目が届いても、期限の時刻で終了する。**主のスレッドは、状態表と記録の時刻のために時計を読まない**（6.4 の順序の決まり）。処理が遅れて流れがたまっても、当てる順と時刻は変わらない。
- 主のスレッドは、チャンネルを `recv_timeout`（1 秒）で待つ。時間切れ（読み取りのスレッドが止まっている）のときは、画面に「ドングルの読み取りが止まっています」を出し、サーバーを確かめる。状態表は進めない（進めるのは流れの時刻だけ）。
- 試験では、`Clock` と読み取りの部分を差し替え、`reader_loop` と `main_loop` を別々に動かせるようにする（主のループの処理の遅れを、流れを先にためてから主のループを動かすことで作る。13 章の M2-5）。

## 11. オーナーに試してもらう手順の案（10 分以内。どれも何も送らない）

**この手順は案。** レビュー担当の承認と PM の確認のあと、**M2-4（共有用の書き出し）と M2-5（Windows の入出力）の両方**（13 章）が main に入ってから、PM が `needs-human` の Issue にする（Issue #6 の決まり）。B と C の記録を渡すには、8.4 の書き出しが要るため。

準備（時間に入れない。PM が段取りする）: 道具の実行ファイルをオーナーの PC に置く。VIVE Hub で 3 台が使える状態にする。HMD は着けなくてよい。SteamVR は閉じておく。

| 試し | 時間 | 手順 | 見てほしいこと | 分かること |
|---|---|---|---|---|
| A 一覧 | 1 分 | 1. VIVE Hub を動かしたまま `openut-dongle list` を実行する。 2. 続けて `openut-dongle check` を実行する | 1 の出力（ドングルの行が出るか）。2 で「サーバーが動いているので始めない」と出るか | U-3、DG-3 の断り |
| B 前提の確認 | 2 分 | 1. VIVE Hub の画面で、3 台が追跡中であることを確かめる。 2. トラッカーを動かさずに `openut-dongle probe-sharing --utc-offset +9` を実行する（初めの 30 秒は開かずに待ち、続く 30 秒だけ共有で開いて読む。画面に 2 つの窓の時刻が出る）。 3. 終わったら、VIVE Hub の画面で 3 台がまだ追跡中かを見る | 2 の出力（開けたか、件数、2 つの窓の時刻）。3 の結果 | 7.3 の B-1（開けたか）、B-2（こちらで読めたか。U-2）、B-3（VIVE Hub 側の途切れが増えないか。U-2、U-17。2 つの窓の VIVE Hub のログをくらべる） |
| C サーバーを止めて読む | 5 分 | 1. 3 台を追跡中のまま、VIVE Hub を終了する（タスクトレイから）。終了した時刻を分まで書き留める。 2. タスク マネージャーで `ViveTrackerServer.exe` が消えたかを見る。残っていれば「タスクの終了」をする（PC のプログラムを止めるだけで、機器への命令ではない）。立ち上がり直したら、そのことを書いて C をやめる。 3. すぐに `openut-dongle record --minutes 3` を実行する。 4. 1 台（足のどれか）を手に取り、30 cm ほど動かして戻す。 5. 1 分たったら、ホストでない足の 1 台の電源を切り、10 秒後に入れ直す。入れ直した時刻を書き留める。 6. 3 分で自動で止まるのを待つ | 1 と 5 の時刻。2 の結果。3〜6 のあいだの画面の行（件数、追跡の状態、位置が動いたか）。5 で「外れた」「つながった」が出たか | 2 つの別の観測に分ける: **C-1 停止後の保持**（U-1 の (b)。止めたあとも、追跡していた台から姿勢と状態が届き続けるか。1〜4 と、5 で電源を切らなかった 2 台で見る）、**C-2 停止中の起動**（U-1 の (a)。止めているあいだに電源を入れた台から、ペアリングの知らせ、文字列の知らせ、姿勢が届くか。5 の台で見る）。ほかに U-4〜U-9、U-11〜U-16 |
| D 後片付け | 1 分 | 1. VIVE Hub を起動し、3 台が追跡を始めるかを見る。始まらなければ、いつもの手順で電源を入れ直す | 始まったか | 試しの影響が残らないこと |

報告: A と B の画面の出力を貼る（個人の情報は出ない作り）。B と C の記録は、`openut-dongle export --anonymize` を通したもの（と、書き出しの終わりに出る、入れなかった報告の件数）と、`summary` の出力を、オーナーの PC につながった Claude のチャットで PM に渡す（クラウドの担当はオーナーの PC を読めない。OPERATING.md の 7 章）。集計をファイルに残すときは、9.1 の `--out` を使う。あわせて、試しの時間帯の VIVE Hub のログ（#7 の書式で匿名化したもの）も渡す。記録そのもの（8.3）は渡さない。

結果は、観測ごとに分けて PM に渡す（どれか 1 つから、ほかは言えないため）。

- **B**: B-1、B-2、B-3 を別々に書く（7.3 の表）。B-3 は、基準の窓と試しの窓の VIVE Hub のログをくらべた数と、試しのあとの VIVE Hub の画面。B-3 を確かめられなかったときはそう書き、「VIVE Hub と並べて読む」（PR #11 の未解決 2）の判断材料にしない。
- **C**: C-1 と C-2 を別々に書く。姿勢が届かなかったときは、どちらで届かなかったかを分ける。C-1 は、トラッカーごとに、止めた時刻（1 の時刻）から最後の姿勢までの秒数と、その間の状態の知らせの有無（`summary` の受信中の区間で分かる）。C-2 は、入れ直した時刻（5 の時刻）のあとに、その台から何が届いたか（ペアリングの知らせ、文字列の知らせの種類、姿勢）。どちらで届かないかで、PR #11 の未解決 1 の選び方が変わる（C-1 で止めた直後の分だけ届くなら (a) の「止めた直後に届く分」が使える。C-1・C-2 のどちらでも届かないなら、M2 では姿勢を読めない）。

危険の見込み: どの試しも、ドングルとトラッカーに何も送らない。B では、VIVE Hub が使っているドングルを、こちらも共有で開いて読む。こちらの読み取りが VIVE Hub の受け取りを減らす作りだった場合（U-2）、30 秒のあいだ VIVE Hub の追跡が乱れうる（装着していないときに行う。3 で確かめる）。C では、追跡中にサーバーを止めるので、トラッカーが待機や切断に入りうる（U-13）。D で元に戻す。

## 12. 仮の値と決め方

| 値 | 仮の値 | 決め方 |
|---|---|---|
| 途切れたとみなすまでの時間 | 2.0 秒 | 試し C の記録で、トラッカーごとの受信の間隔の分布を出し、ふつうの最大の間隔の 4 倍（ただし 1 秒以上）にする |
| 表示の間隔 | 1 秒 | 変えない |
| 姿勢の毎秒の件数の窓 | 直前 1 秒 | 変えない |
| サーバーの見張りの間隔 | 2 秒 | 変えない（プロセスの一覧を取る手間は小さい） |
| `record` の長さ | 既定 10 分、最大 60 分 | 試し C の記録の大きさを見て、PM が見直す |
| `probe-sharing` の長さ | 試しの窓 30 秒（最大 60 秒）。基準の窓も同じ長さ | 変えない。B-3 で差が言い切れなかったら、PM が 60 秒での取り直しを決める |
| 読み取りの待ち（`Idle` の間隔。報告がないときに `advance` を呼ぶ間隔） | 100 ms | 変えない |
| 主のスレッドの受け取りの待ち（読み取りが止まったとみなして画面に出すまで） | 1 秒 | 変えない（表示だけで、状態表には使わない） |
| `tick` の行の間隔 | 1 秒 | 変えない |

## 13. 実装の項目（PR 1 本の大きさ）

**この章の項目（M2-1〜M2-6）は、`implementation-plan.md` の 4 章に移した**（Issue #57。番号、範囲のファイル、受け入れ条件、順序の決まりは、移したときのまま）。これからの直しは、`implementation-plan.md` で行う。

## 14. M3 への引き継ぎ

- そのまま使うもの: 記録の形（M3 の再生テストの入力）、解読、トラッカーの状態表、匿名化した書き出し、サーバーの見張り（M3 でも、サーバーが動いていたら起動を断る）。
- M3 で足すもの: 送る部分（`commands.md` の 6 章の形）、M1 の状態と検出の規則への差し替え（R-9。`m2-dongle-protocol.md` の 8 章の表を、試し B・C の記録で確かめてから）、SteamVR への受け渡し。
- 試し C で、PC が送らないと姿勢が届かないと分かった場合、M3 の最初の仕事は、追跡を始める命令（`commands.md` の 4.1）の確かさを上げることになる。
