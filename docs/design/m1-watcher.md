# M1 見張り役の設計

VIVE Hub を使ったまま横で動かし、VIVE Hub のトラッカー用サーバーのログを読んで、始まらない・途切れる理由と対処を、その場で知らせる（PRD の 5 章）。最終更新: 2026-11-28（ログの書式のメモに合わせた追補。Issue #18。PR #26 のレビューの指摘への対応を含む）。

## 0. 範囲と前提

- 根拠: `docs/product/PRD.md`（4〜6 章、11 章）、`docs/project/DECISIONS.md`（D-9〜D-33）、`docs/research/findings-2026-11-27.md`、`docs/research/findings-2026-11-28.md`、`docs/research/hub-log-format.md`（書式のメモ。以下「メモ」）、`docs/research/hub-log-samples.md`（見本）、`overview.md`。
- **メモ（Issue #7）に合わせて直した版**（Issue #18）。直したのは、4.1〜4.8（読み取り、行の種類）、5 章（状態）、6.2 の R1 と R4、8 章（集計）、9 章（設定、フォルダー）、10 章（再生）、14 章。メモは、オーナーの PC 1 台・6 日分・1 つの版のログによる（メモの冒頭）。ほかの版や PC で違っていても止まらないことは、4.7 で守る。
- PR #26 のレビュー（1 回目）の指摘で直したもの: ずれの読めない行の時刻（4.3 の「ずれの補い」）、ドライバーの記録を待つ決まり（3.7、4.6。R4 の照合の範囲と、追記を待つ時間を分けた）、予告と接続の対応（6.2 の R4）、ホストの根拠を Wi-Fi 情報と別に持つこと（3.5、5.3）、ドングルが抜けたときにドングル経由の台だけを切ること（5.1。接続の経路 `link`）。
- レビュー（2 回目）の指摘で直したもの: サーバーとドライバーで時間の基準を 1 つにすること（4.3 の「ずれの補い」）、起動・再構築の追いつきの終わりでも、追記中のドライバーのファイルを待つこと（4.5、4.6 の 3。d₀）、切断の行を見落としたつなぎ直しで接続を区切ること（5.1 の「接続」、6.2 の R4）、ホストの根拠を接続に結びつけ、番号が別の台に使われたら、その台の名前で案内しないこと（3.5、5.3、6.2 の R1）。
- **ドライバーのファイルも読む**（4.1）。サーバーのファイルにない「電源オフの予告」（メモの 3.5）を、R4 の除外に使うためだけ。PRD の W-1 は、サーバーのファイルを挙げている（読むものを足すだけで、W-7 の「読むだけ」は変わらない。14 章で PM に確かめる）。
- M1 は実機に命令を送らない（W-7、D-13）。読むのはログのファイルだけ。書くのは、OpenUT 自身の保存場所のファイル、デスクトップの通知、SteamVR の中の表示だけ（11 章）。
- トラッカー内のログの収集（L-3）は、この文書の範囲に入れない。オーナーの PC のつなぎのスクリプトを確かめてから、別に依頼が出る（STATUS の「次の一手」の 6）。
- 画面と通知の**見た目**は UI/UX デザイン担当の範囲（`docs/design/ui/`）。この文書は、何をいつ見せるか、案内の文言の意味、操作の流れまでを決める（7 章）。
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
| 見張り | 500 ms ごとに追記を読み、エンジンに渡し、出力を配る。集計を 60 秒ごとと日の終わりに書く |
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
/// Not stable across reconnections: never use it to name a tracker.
pub struct TrackerKey(pub u16);
/// A serial number, when the source tells it. Never written to the repository.
pub struct Serial(pub String);
```

- **番号 N はドングルのスロット**（0〜4）で、つなぎ直しで変わる。同じトラッカーが、同じ日に別の番号で出る（メモの 3.5）。
- 状態と規則は、出来事が持つ番号（`TrackerKey`）をキーにした表で動く（どの出来事にも番号が入り、シリアル番号が入るのは 1 種類の行だけのため）。台数を決めない（W-9）。初めて出てきた番号で行を足す。
- **トラッカーそのものは、シリアル番号で見分ける**。番号とシリアル番号の対応は `SerialKnown`（4.4 の `GetDeviceInfoAndResponse` の行。接続のたびに何度も出る）で分かり、その番号の行に付ける（5.1）。利用者に見せる名前と、日ごとの集計の行は、シリアル番号で引く（8 章、9.1）。
- 名前は、設定のニックネーム（9.1。シリアル番号で引く）。シリアル番号がまだ分からない番号と、ニックネームのないトラッカーは `トラッカー{N}`（N はいまの番号）。番号で名前を引くことはしない（つなぎ直すと、別のトラッカーを指すため）。

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
pub enum LineKind { ServerStart, ServerRelease, DongleRemoved, TrackerAdded, TrackerRemoved,
    DeviceInfo, HostMode, HostWifiQuery, HostWifiGot, WifiAssign, WifiNotYet, ClientLink,
    DeviceStatus, Pose, OetLost, OetReloc, OetOverspeed, Command, PowerOffNotice, Other }
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
    DongleRemoved,                                            // every tracker connected through the dongle is gone
    TrackerAdded { tracker: TrackerKey, usb: bool },
    TrackerRemoved { tracker: TrackerKey, usb: bool },        // usb: connected by cable, not by the dongle
    SerialKnown { tracker: TrackerKey, serial: Serial },
    HostAssigned { tracker: TrackerKey },                     // PC told the tracker to be the host
    HostWifiQueried { host: TrackerKey },                     // PC asked the host for its Wi-Fi info
    HostWifiObtained { host: Option<TrackerKey> },            // PC got the host's Wi-Fi info (M1: None)
    WifiRequestUnanswered { tracker: Option<TrackerKey> },    // a client asked, PC had nothing (M1: None)
    WifiAssigned { tracker: TrackerKey },                     // PC gave the info to a client
    ClientLink { tracker: TrackerKey, connected: bool },      // client joined / left the host's Wi-Fi
    Status { tracker: TrackerKey, connected: bool, host: bool, map_ready: bool,
             usb: Option<bool> },                             // a report, not a change. usb: None if not written
    Pose { tracker: TrackerKey, status: PoseStatus },
    Oet { tracker: TrackerKey, kind: OetKind },
    PowerOffNotice { tracker: TrackerKey },                   // the tracker said it is powering off
    PcCommand { tracker: Option<TrackerKey>, kind: String },  // other PC -> tracker commands. None: all
}
pub enum PoseStatus { Ok, Lost, RecentlyLost, Other(i64) }    // 2, 3, 4
pub enum OetKind { LostTracking, RelocBack, Overspeed }
```

- `HostAssigned`、`HostWifiQueried`、`WifiAssigned` は、PC からトラッカーへの命令でもある。R4 の「直前に命令があったか」は、この 3 つと `PcCommand` を見る。`PcCommand` の `tracker: None` は、番号のない命令（`Unpair All` など）で、すべてのトラッカーへの命令として扱う。
- `Status` は、トラッカーごとの状態の報告（4.4 の `DeviceStatusToVHServer` の行）。変わったときだけでなく、数分おきにも出るので、状態を変える出来事ではなく、いまの様子の報告として扱う（5.1）。`host` はホストの根拠（5.3）、`usb` は接続の経路（5.1。`TrackerAdded` を見ていない台のため）に使う。
- `TrackerAdded` と `TrackerRemoved` の `usb` は、`connection type: 2`（USB でつないだ。推定。メモの 3.5）のとき `true`。ドングルを通さない接続なので、`DongleRemoved` では切れない（5.1）。
- `HostWifiObtained` と `WifiRequestUnanswered` の行には、トラッカーの番号がない（メモの 3.1）。M1 では常に `None` で、5 章の「番号のない出来事」の扱いになる（M3 のランタイムは番号を持てるので、型には残す）。
- 初版の `Map` の出来事は、`Status` の `map_ready` に置き換えた（地図の状態は単独の行ではなく、状態の行の一部のため。メモの 3.1）。

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
pub struct Snapshot {
    pub trackers: Vec<TrackerView>, pub host: Option<HostRef>, pub host_wifi: HostWifi, pub at: LogTime,
}
```

- **ホスト**（`host`）と **PC の Wi-Fi 情報**（`host_wifi`）は別に持つ。途中から読み始めると、Wi-Fi 情報は `Unknown` のままでも、状態の行の `is host: 1` でホストは分かるため（5.3）。R1 の案内するトラッカー（6.2）は `host` を使い、番号のない問い合わせの候補（5.1）は、`host` から決まる役割を使う。
- **ホストの根拠は、番号だけでなく接続（`conn`）に結びつける**（5.3）。番号はスロットで、ホストが切れたあとに別のトラッカーが同じ番号でつながりうるため。「ホストの接続が続いている」は、番号 `key` の行が未接続でなく、その行の `conn` が `HostRef::conn` と同じこと。続いていなければ、その番号の台をホストとして案内しない（6.2 の R1）。状態の窓のホストの印（7.4）も、接続が続いているときだけ付ける。

画面に出す状態の名前（W-2）: `Disconnected` 未接続、`Connected` 接続、`WaitingWifi` Wi-Fi 待ち、`MapSyncing` 地図同期中、`Relocalizing` 位置取り直し中、`Tracking` 追跡中。PC の Wi-Fi 情報: `Held` 持っている、`Missing` 持っていない、`Unknown` 不明。

### 3.6 検出と案内（`openut_core::rules`、`openut_core::notice`）

```rust
pub enum RuleId { R1, R2, R3, R4, R5 }
/// One run of a rule's condition, from start to end.
pub struct Detection {
    pub rule: RuleId, pub tracker: Option<TrackerKey>, pub episode: u64,
    pub at: LogTime, pub step: Step,
}
pub enum Step { Raised, Escalated, Resolved, Ended }   // Ended: the condition went away without a fix
pub struct Notice {
    pub rule: RuleId, pub tracker: Option<TrackerKey>, pub episode: u64,
    pub step: Step, pub at: LogTime,
    pub headline: String,   // state, one short line (Japanese)
    pub action: String,     // what to do now, one line (Japanese). Empty for Resolved.
    pub push: bool,         // true: show in VR and on the desktop. false: status window only.
    pub priority: u8,       // R1 = 5 (highest) ... R5 = 1
}
```

- 規則は `Detection` を出す。配信の決まり（7.1）が、`Detection` から `Notice` を作り、`push` を決める。どちらも入出力を持たず、単体テストで確かめる。
- 文言（7.1 の表）は `openut_core::text` にまとめ、トラッカーの名前を差し込む関数にする（テストで文言を確かめられるように）。

### 3.7 エンジン（`openut_core::engine`）

```rust
impl Engine {
    pub fn new(cfg: EngineConfig, names: Names) -> Engine;
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
  - 見分け方の 3（消えた）のときは、残っているうちで最も新しいものを、いまのファイルにして再構築する。1 つもなければ「ログが見つからない」（7.4 の見張り役の状態）にして、見つかったら再構築する。
  - 再構築したあとの集計は、起動し直した場合と同じ（いまあるファイルから決まる。8.1）。再構築の回数を `source.rebuilds` に数え、アプリのログに理由（縮んだ、置き換えられた、消えた）を残す。
  - サーバーとドライバーの、どちらのいまのファイルが作り直されても、両方を 4.5 の手順で読み直す（決まりを 1 つにするため。6 日分では、作り直しは起きていない。メモの 2.3 の `Truncate=0`）。ドライバーのファイルが消えて 1 つも残らないときは、再構築せず、ドライバーなしで続ける。
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
| DongleRemoved | `OETServer::OnDongleRemovedCB` | `Dongle is removed.` | — | `DongleRemoved` |
| TrackerAdded | `OETServer::AddTrackerCB` | `Add new tracker: (N), connection type: V` | N、V | `TrackerAdded`（`usb` は V が 2 のとき） |
| TrackerRemoved | `OETServer::RemoveTrackerCB` | `Remove tracker (N), connection type: V, power state:V `（`power state:` のあとに空白がない） | N、V（connection type） | `TrackerRemoved`（`usb` は V が 2 のとき） |
| DeviceInfo | `OETServer::GetDeviceInfoAndResponse` | `Response - cmd type: ` で始まり、`, sn: (` を含む（含まなければ `Other`）。`, device index: N,` と `, sn: (S)` | N、S（括弧の中。空なら出来事にしない） | `SerialKnown` |
| HostMode | `Tracker::UpdateSwitchingMode` | `Tracker (N) - switch to HOST mode.` | N | `HostAssigned` |
| HostWifiQuery | `OETServer::TrackerSwitchAndResponse` | `Query wifi from tracker N`（括弧なし） | N | `HostWifiQueried` |
| HostWifiGot | `OETServer::ProcessDeviceMsg` | `Got host wifi info.`（**番号なし**） | — | `HostWifiObtained { host: None }` |
| WifiAssign | 同上 | `Assign wifi info to client tracker (index: N).` | N | `WifiAssigned` |
| WifiNotYet | 同上（`WARN`） | `Wifi info is not yet available.`（**番号なし**） | — | `WifiRequestUnanswered { tracker: None }` |
| ClientLink | 同上 | `Client tracker (index: N) is connected to host: V` | N、V（0 か 1。ほかは読めなかった行） | `ClientLink`（V が 1 なら `connected`） |
| DeviceStatus | `OETServer::DeviceStatusToVHServer` | `display index: ` で始まり、`key: value` を `, ` で並べたもの（下の注） | `device index`、`connected`、`map status`、`map state`、`is host`、`bIsConnectedOnUSB`（任意） | `Status` |
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
- **状態の行**（DeviceStatus）: トラッカー 1 台について 1 行（メモの 3.1）。`, ` で区切って `key: value` の組にし、上の 5 つのキーの値を整数で読む（どれかがない・読めなければ、読めなかった行）。ほかのキー（`tracking status`、`percentage` など）は使わない。出来事の値:
  - `connected`: `connected` が 1。`host`: `is host` が 1。
  - `usb`: `bIsConnectedOnUSB` が 1 なら `Some(true)`、0 なら `Some(false)`、キーがない・読めなければ `None`（このキーだけは、なくても読めなかった行にしない。接続の経路を補うだけのため。5.1）。見本の状態の行は、どれも 0。
  - `map_ready`: `map state` が 2、または `map status` が 6。値の意味は、初版の調べ（解析ライブラリ pyvut の定数の名前 `MAP_REUSE_OK` などと合う、という記述）による。pyvut（<https://github.com/rocketmark/pyvut>）にはライセンスの表示がないので、定数の表は写さず、意味だけを使う。メモでは、意味を確かめ直していない。
  - `tracking status` は使わない。姿勢の行の値（2・3・4）にない 5・6・7 が出るので、同じ番号づけか分からないため（メモの 3.1）。
- **姿勢の行**（Pose）: 状態が変わったときだけ出る（`Pose status is changed:`。定期的には出ない）。`V` には、接続の直後に意味のなさそうな値（47、58、71 など）や 0 が出る。どれも `Other(V)` にし、止まらない。位置や回転の `-nan` と桁の多い値は読まない（判定に使わない）。
- 本文の先頭が `(SteamVR: ` でも、`]Pose status is changed: Pose data - status: ` を含まない行（`... includes NAN.`。1 日に最大 8,646 行）は `Other`。
- **数が多くて検出に使わない行**（メモの 3.1 の終わり）: `... includes NAN.`、`OnPipeClientError::OnPipeError ...`、`Tracker is not found`。どれも `Other`。サーバーの行は 1 日に最大 15,828 行（毎秒 1 行に満たない）なので、読み飛ばしの特別な仕組みは作らない。判定は、本文の先頭の 1 文字か 2 文字で候補を絞り、表の形と前から比べるだけにする（正規表現のクレートを足さない）。
- 番号や値を読むのは、表の形の `N`・`V` の位置だけ。形の先頭が合ったのに、決まった位置の数が読めない（桁あふれ、数でない）ときは、読めなかった行（`unparsed`。4.7）にする。番号が 0〜4 の外でも読めた数なら受け付ける（台数を決めない。W-9）。
- **同じ出来事の行が 2 回出る**（メモの 2.2。例: `Remove tracker (2), ...` が 3 ミリ秒おいて 2 回）。本文が同じ行を捨てる処理は**しない**（続けて来る `Wifi info is not yet available.` は、1 つ 1 つが別の問い合わせで、捨てると R1 が数え違えるため）。重なりは、状態の側で、同じ遷移を 2 回しないことで吸収する（5.1）。
- `Dongle is added.`、`Tracker N entered sleeping (lost tracking)`、`SteamVR Running is changed to V`、`error code: V received` などは、M1 の規則に使わないので `Other`（数えるだけ）。

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

- 数は、日ごとの集計（8 章）の `source` に書く。`unparsed` が増えたら、ログの書式が変わった印なので、状態の窓に「ログの形が想定と違う行が N 行」と出す（案内ではなく、見張り役の状態として。7.4）。
- **綴りが変わった版**では、表の行がすべて `other` になり、`unparsed` は増えない。そのため、その日のサーバーの記録が 2,000 件（仮）を超えたのに、表の種類（`Other` でないもの）が 1 件もなければ、同じく「ログの形が想定と違う」にする（7.4）。6 日分では、1 日の記録は 2,002〜15,828 件で、状態の行（`DeviceStatusToVHServer`）がトラッカーをつないでいない日にも出る見込み（起動の直後に、登録済みのトラッカーの行が出る。メモの 3.5）。
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
| U-17 | ほかの版（VIVE Hub、ランタイム、ファームウェア）と、ほかの PC で、綴りと形が同じか。メモは 1 台・6 日・1 つの版 | 4.4。変わったら 4.7 の「ログの形が想定と違う」で気づく | ログの送信（L-4）が入ってから |

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
| `SerialKnown(N, S)` | 未接続以外 | 変えない | N のシリアル番号を S にする。ほかの番号に S が付いていれば、そちらから外す |
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
| `Pose(N, Ok)`、`Oet(N, RelocBack)` | 未接続以外 | 追跡中 | `tracked_since_connect = true` |
| `Pose(N, Lost か RecentlyLost)`、`Oet(N, LostTracking)` | 追跡中、地図同期中 | 位置取り直し中 | |
| `Pose(N, Lost か RecentlyLost か Other)` | 接続、Wi-Fi 待ち | 変えない | 数えない（接続の直後に、意味のなさそうな値や `status: 3` が出るのはふつう。メモの 3.1） |
| `Pose(N, Other)`、`Oet(N, Overspeed)` | そのほか | 変えない | 集計に数える |
| `PowerOffNotice(N)` | — | 変えない | R4 が振り返って使う（6.2）。状態は、そのあとの `TrackerRemoved` で変わる |
| `PcCommand` | — | 変えない | R4 が振り返って使う |
| `DongleRemoved` | 未接続以外で、`link` が `Usb` でない台 | 未接続 | 役割とシリアル番号をなしに、`link` を `Unknown`。ドングルが抜けると、ドングル経由の台の `Remove tracker` が出ないため（メモの 3.5。1 回の観察。U-12）。**`link` が `Usb` の台は変えない**（ドングルを通さない接続のため。シリアル番号と、集計の接続の時間も続く）。`link` が `Unknown` の台は、ドングル経由として切る（6 日分の切断の 109 件が、ドングル経由〔`connection type: 1`〕のため） |
| `ServerStarted` | どれでも | 全台を未接続 | 役割とシリアル番号をなしに、`link` を `Unknown`。PC の Wi-Fi 情報を `Missing`（5.2）、ホストをなしに（5.3）。終了の行がないまま起動し直すこともあるので（メモの 3.5）、ここで必ず捨てる |
| `ServerStopped` | どれでも | 全台を未接続 | 役割とシリアル番号をなしに、`link` を `Unknown`。PC の Wi-Fi 情報を `Unknown`、ホストをなしに（5.3） |

- 表にない組み合わせ（例: 未接続のトラッカーの `Pose`）は、状態を変えずに「順序の合わない出来事」として数える。ただし、`TrackerAdded` を一度も見ていない番号（途中から読み始めた）は例外で、その出来事から推し量った状態で行を足し、`inferred = true` にする（`Pose(Ok)` と `Oet(RelocBack)` なら追跡中、`Pose(Lost か RecentlyLost)` と `Oet(LostTracking)` なら位置取り直し中、`ClientLink(true)` なら地図同期中、`Status(connected, map_ready)` なら位置取り直し中、`Status(connected)` なら接続、など。推し量れない出来事なら「接続」。`Status` の `connected: 0` と `SerialKnown` では足さない）。推し量って足した行の `tracked_since_connect` は `false` から始める（追跡中で足したなら `true`）。`link` は、足した出来事が `Status` で `usb` があればその値、なければ `Unknown`（あとの `Status` で決まる）。推し量って足した行にも、新しい `conn` を付ける（下の「接続」）。
- `Status` の 4 つの行は、1 つの状態の行に、当てはまるものをすべて当てる（状態は上の 2 行のどちらかで決め、`link` とホストは、それぞれの行で決める）。
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
  1. ホストの接続が続いていれば、その台（番号 `key`）。名前は、その番号に付いたシリアル番号で引く（3.2）。
  2. 接続の終わったホストで、根拠の `serial` が分かっていれば、同じシリアル番号の台を探す。いまどこかの番号 M でつながっていれば、その台（番号 M。つなぎ直して番号が変わった同じトラッカー）。つながっていなければ、そのシリアル番号のニックネーム（9.1）があれば、それ（同じトラッカーと分かるため）。
  3. どれでもなければ（ホストがない、接続の終わったホストのシリアル番号が分からない、ニックネームがない）、「ホストのトラッカー」。**同じ番号にいまつながっている別の台の名前（`トラッカー{N}` も）は使わない**（レビュー〔2 回目〕の指摘。番号の使い回しで、別の台の電源を案内しないため）。
  - `Detection` の `tracker`（3.6）は、1 か 2 で台が決まったときはその番号（1 は `key`、2 は M）、それ以外は `None`。`expect.toml` の `tracker`（10.3）は、この値と比べる。
  - **例**（5.3 の例の続き。A のシリアル番号は分からない）: B が番号 2 でつながっているあいだに R1 が出ると、案内は「ホストのトラッカーの電源だけを入れ直す」で、`tracker` は `None`。A のシリアル番号が分かっていて、ニックネームが「腰」なら「腰」。A が番号 4 でつなぎ直していれば（同じシリアル番号の `SerialKnown(4, …)`）、A の名前で、`tracker` は 4。
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
| ログが止まった（伸びない） | 見張り役の状態として出す（7.4） | トラッカーの不具合ではなく、見張り役が見えていないこと |

## 7. 案内の出し方（W-5）

### 7.1 配信の決まりと文言

配信の決まり（`openut_core::notice`。入出力なし）は、`Detection` を受けて `Notice` を作る。`push = true` の案内だけが、SteamVR の中とデスクトップに出る。すべての案内は、状態の窓の案内の欄に、エピソードが終わるまで出る。

| 規則 | `Raised` | `Escalated` | `Resolved` | `Ended` | 同じ案内を繰り返さない決まり |
|---|---|---|---|---|---|
| R1 | push | push（1 回だけ） | push | 欄から消すだけ | エピソードごとに 1 回。間隔は空けない（11/27 は 3 分おきに 3 回起きており、毎回知るべきため） |
| R2 | push | — | 欄から消すだけ | 欄から消すだけ | 同じトラッカーの R2 のエピソードが終わってから 5 分（`notice.cooldown_s`）以内に始まったものは push しない |
| R3 | push | — | 欄から消すだけ | 欄から消すだけ | R2 と同じ |
| R4 | push | — | 欄に「{T} がつながりました」を 30 秒出してから消す | 欄から消すだけ | 同じトラッカーの R4 を push してから 5 分以内は push しない（欄の案内に回数を足す） |
| R5 | push | — | 欄から消すだけ | 欄から消すだけ | R2 と同じ。**既定では R5 を切ってあるので、何も出ない**（6.2） |

全体の決まり:

- **間隔**: push は、前の push から 10 秒（`notice.min_gap_s`）以上空ける。そのあいだに来た push は待たせ、まとめて 1 つにする（優先度の最も高いものを出し、見出しに「ほか N 件」を足す）。優先度は R1 > R4 > R3 > R2 > R5。
- **追いつきのあいだ**（4.5）: push しない。終わった時点で続いているエピソードを、優先度の順に、上の間隔の決まりで出す。
- **再構築のあと**（4.1）: 配信の決まりは、再構築の前に push 済みで続いていたエピソードを、規則とトラッカーの組で覚えている。再構築の追いつきが終わった時点で続いているエピソードのうち、規則とトラッカーが同じものは、同じエピソードの続きとみなして push し直さない（push 済みの段階も引き継ぐ。R1 で `Escalated` を push 済みなら、もう出さない）。覚えている組のうち、再構築のあと続いていないものは、欄から消すだけ（R1 の「直りました」も出さない。直ったのを見たのではないため）。エピソードの番号は再構築で振り直されるので、組の照合には使わない。
- アプリを起動し直したときは、配信の決まりの覚えていることもなくなるので、続いているエピソードはもう一度 push される（起動し直した利用者に、いまの問題を知らせ直すのはよいこととする）。
- **直ったら待たせている案内を捨てる**: 待たせている push のエピソードが、出す前に `Resolved` か `Ended` になったら、出さない。
- 配信の値（`notice.cooldown_s`、`notice.min_gap_s`）も仮の値。再生テストで、1 晩に出る push の数を見て決める（6.1）。

文言（意味。見た目と言い回しの調整は UI/UX デザイン担当。意味を変えるときは設計の変更）。`{T}` は対象のトラッカーの名前、`{H}` はホストの名前（3.2。決め方は 6.2 の R1 の「案内するトラッカー」。決まらなければ「ホストのトラッカー」）。オーナーの方針（`roles/ux.md`）に合わせ、見出し（状態）と、いま何をすればよいかの 1 行だけにする。

| 規則と段階 | 見出し | すること（1 行） |
|---|---|---|
| R1 `Raised` | トラッカーが始まらない状態です | {H} の電源だけを入れ直す。ほかのトラッカーとドングルはそのまま |
| R1 `Escalated` | まだ始まらない状態です | {H} の電源だけを入れ直す。ほかのトラッカーとドングルはそのまま |
| R1 `Resolved` | 直りました | （なし） |
| R2 `Raised` | {T} が地図を受け取れていません | {T} を {H} の近くに置いて待つ |
| R3 `Raised` | {T} の位置合わせが終わりません | {T} を手に持ち、部屋の中をゆっくり見せる |
| R4 `Raised` | {T} の接続が切れました | {T} の電源が入っているか確かめる |
| R4 `Resolved` | {T} がつながりました | （なし） |
| R5 `Raised` | {T} が 1 分以上、位置を失っています | {T} のカメラの前をあけ、明るい方へ向ける |

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
- **文字の描画**: 書体は、実行時に Windows の書体のフォルダーから読む（レポジトリと配布物に書体を入れない）。候補は Yu Gothic UI、メイリオ、MS ゴシック の順。日本語以外の Windows にこれらがあるかは未確認。どれもなければ、SteamVR の中の表示をやめ、デスクトップと状態の窓だけにする（状態の窓に理由を出す）。描画のクレートは `fontdue` 0.9.5（MIT OR Apache-2.0 OR Zlib）か `ab_glyph` 0.2.32（Apache-2.0）。状態の窓（egui）と同じものにできるなら、そろえる（M1-12 で決める）。
- 毎フレームの描画はしない。画像を渡すのは案内を出すときだけ（12 章）。

### 7.3 デスクトップ

- **方式**: Windows のトースト通知。クレートは `tauri-winrt-notification` 0.8.1（MIT OR Apache-2.0。tauri-apps。2026-07 に更新）。
- 見出しと 1 行を、そのまま 2 行にして出す。音は既定のまま。トーストを押したら状態の窓を開く（押したことを受け取れるかは未確認。受け取れなければ、トレイから開く）。
- トーストには、送り主のアプリの ID（AppUserModelID）が要る。クレートの説明では、インストールしていないプログラムは、当面 PowerShell の ID（`Toast::POWERSHELL_APP_ID`）を使えるとある。その場合、送り主が「Windows PowerShell」と表示される。OpenUT 自身の ID を登録する方法（スタートメニューのショートカットか、レジストリ）は、Microsoft の公式の文書をこの環境から読めなかったので未確認。M1-10 で確かめ、確かめられるまでは PowerShell の ID を使う。
- SteamVR が動いているときも出す（W-5 は両方に出すことを求めている。設定でどちらも切れる）。

### 7.4 画面: 何をいつ見せるか

画面は 3 つ: トレイのアイコン（常駐）、状態の窓（開いたときだけ）、通知（7.2、7.3）。画面なしで動かす `openut watch`（コンソールに案内を出す）もある（M1-9）。

**見張り役の状態**（トラッカーの状態とは別。トレイと状態の窓に出す）

| 状態 | 条件 |
|---|---|
| 見張り中 | いまのファイルを読めている |
| 待機中 | 見張り中で、未接続以外のトラッカーがない |
| ログが見つからない | ログのフォルダーか `ViveTrackerServer_*.txt` が見つからない |
| ログを読めない | 読み取りが 3 回続けて失敗した（共有の違反、権限など。理由を出す） |
| ログが止まっている | 未接続以外のトラッカーがあるのに、10 分（仮）ファイルが伸びない |
| ログの形が想定と違う | 見張り中のまま、その日の `unparsed` が 100 行（仮）を超えた。または、その日のサーバーの記録が 2,000 件（仮）を超えたのに、4.4 の表の種類が 1 件もない（4.7） |

**トレイのアイコン**

- 3 つの見え方: ふつう（見張り中・待機中で、案内の欄が空）、案内あり（案内の欄に 1 件以上）、見えていない（ログが見つからない・読めない・止まっている）。
- 指を置いたときの 1 行: 「追跡中 2/3」（追跡中の台数／未接続以外の台数）、「案内 1 件」、「ログが見つかりません」など、いまの見え方の理由。
- メニュー: 「状態を開く」「終了」。左クリックでも状態の窓を開く。

**状態の窓**（小さな窓。閉じてもアプリは終わらない。トレイから開く）

上から:

1. 見張り役の状態の 1 行（例: 「見張り中（11/28 のログ）」）。「ログが見つからない」ときは「フォルダーを選ぶ」ボタン。
2. 案内の欄: 続いているエピソードの見出しと 1 行。新しいものが上。なければ欄ごと出さない。
3. トラッカーの一覧: 名前、ホストの印（ホストの接続が続いている台だけ。3.5）、状態（W-2 の 6 つの名前）、その状態になってからの時間。行の並びは番号の順。
4. PC の Wi-Fi 情報: 持っている／持っていない／不明。
5. 今日の集計: 位置が出ていた割合、喪失の回数、不具合の回数（8 章）。
6. 設定: 通知（VR の中、デスクトップ）を出すかどうか。

**操作の流れ**

| 流れ | 手順 |
|---|---|
| F1 起動 | 起動 → ログのフォルダーを探す（9.2）→ 見つかれば、窓を開かずにトレイに常駐し、追いつき（4.5）のあと見張り中。見つからなければ、状態の窓を開いて「フォルダーを選ぶ」を出す |
| F2 案内 | push の案内 → VR の中とデスクトップに出る → 状態の窓の案内の欄に残る → 直ったら欄から消える（R1 は「直りました」も push） |
| F3 トラッカーに名前を付ける | 状態の窓の一覧で名前を押して入力する → シリアル番号をキーにして設定に保存（9.1）→ 以後の案内と一覧に使う（つなぎ直して番号が変わっても、同じ名前）。シリアル番号がまだ分からない行（3.2）は、名前を付けられない（押せない。分かるのは、ふつう接続から 1 秒以内。見本の 4 章）。どの行がどのトラッカーか分からないときは、1 台のカメラを手で覆うと、その行が数秒遅れて「位置取り直し中」に変わるので、それで見分ける（覆うと位置合わせがやり直しになることは、HTC の公式の案内「位置が合わないとき: カメラを手で 5 秒覆ってから離す」からの推定。ログに出るまでの遅れは 4.6） |
| F4 フォルダーを選ぶ | 「フォルダーを選ぶ」→ フォルダーの選択の画面 → 選んだフォルダーに `ViveTrackerServer_*.txt` があれば設定に保存して見張り中。なければ、そう出して選び直してもらう |
| F5 終了 | トレイのメニューの「終了」→ 集計を書き出してから終わる |

**画面の作り方（Rust）**

| 部品 | クレート（2026-11-28 に crates.io で確認） | ライセンス | 保守 | 確かめたこと・未確認 |
|---|---|---|---|---|
| トレイ | `tray-icon` 0.27.0（メニューは `muda` 0.22.0） | MIT OR Apache-2.0 | tauri-apps。2026-11-25 に更新 | Windows では、作ったスレッドでイベントループが回っている必要がある（README で確認）。バルーン通知の機能はない（ソースで確認） |
| 状態の窓 | `eframe` / `egui` 0.37.0（内部で `winit` 0.30.13。Apache-2.0） | MIT OR Apache-2.0 | 2026-10-15 に更新 | egui の既定の書体には日本語の字がないので、7.2 と同じく Windows の書体を実行時に読み込む（既定の書体に日本語がないことは、この版では確かめていない）。窓を閉じたときに隠すだけにできるか、トレイと同じイベントループで動くかは、M1-11 の最初に確かめる |
| フォルダーの選択 | `rfd` 0.18.0 | MIT | 2026-11-07 に更新 | |

- 窓は、開いているあいだだけ描く（egui の、入力や更新があったときだけ描き直す動き）。隠しているあいだは描かない（12 章）。
- 候補を比べた結果: `native-windows-gui` は更新が止まっている見込み（この版では確かめていない）、`slint` はライセンスが GPL か独自のもので MIT と組み合わせる条件が複雑、Tauri は WebView を使い M1 の小さな窓には重い。そのため上の組み合わせにした。

### 7.5 UI/UX デザイン担当に頼むこと（PM が `role:ux` の Issue にする）

1. トレイのアイコンの 3 つの見え方（7.4）。
2. 状態の窓の配置（7.4 の 1〜6）と、トラッカーの 6 つの状態の見分け方。
3. SteamVR の中の案内の板: 大きさ、HMD からの距離と高さ、表示の長さ（仮 10 秒）、色、字の大きさ。VR の中で読めること（`roles/ux.md` の 4）。
4. デスクトップのトーストの形（見出しと 1 行）。
5. 7.1 の文言の言い回しの調整（意味は変えない）。

## 8. 日ごとの集計（W-6）

- 集計（`openut_core::summary`。入出力なし）は、行の時刻と、状態の変わり目と検出から、日ごとに数える。日の区切りと数え方は 8.1。
- **同じファイルの組（前の日のファイルと、その日のファイル）から決まる**ようにする。0 時をまたいで動き続けても、起動し直しても（4.5）、作り直しで再構築しても（4.1）、同じファイルを再生しても（10.2）、同じ結果になる（例外は 8.1 の終わり）。過ぎた日の分は `openut summarize` で作り直せる（M1-9）。
- 書き出しはアプリ（M1-9）: 60 秒ごと、日を閉じたとき、終了のときに、一時ファイルに書いてから名前を変える（途中で落ちても壊れたファイルを残さない）。追いつきのあいだは書かない（4.5）。置き場所は 9.3。

### 8.1 日の区切りと数え方

- **日の区切り**: 日 D の分は、D の 0:00:00.000 から、次の日の 0:00:00.000 の手前まで。比べるのは行に書かれた地方の日付と時刻（`LogTime` では、その行のずれで D の 0 時を表した時刻）。ファイル名の日付では分けない（前の日のファイルに 0 時より後の時刻の行があれば、その行は D の分）。ファイル名の日付は、どのファイルを読むか（4.1、4.5）にだけ使う。
- **時間の値**（`connected_s`、`in_use_s`、`tracking_s`、`unobserved_s`）: サーバーのファイルの、先頭が読めた行（種類を問わない。`Other` も含む。ドライバーの行は時刻を進めないので入れない。3.7）と、次の先頭が読めた行のあいだの区間を、前の行を適用したあとの状態で数える。区間が 0 時をまたぐなら、0 時で 2 つの日に分ける。
  - 区間が `summary.gap_s`（仮 600 秒のまま。7.4 の「ログが止まっている」の 10 分と同じ値）より長ければ、その区間は状態に数えず、`unobserved_s` に足す。VIVE Hub が行を書かずに止まったとき（終了の行のない終わり。メモの 3.5）や、PC のスリープ（記録が最長 10.6 時間出なかった）に、止まる前の状態のまま何時間も数えないため。
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
- **同じにならない場合**（どれも `source` か、ここに書いた値で分かる）: (1) 切り替わったあとの古いファイルの書き足し（`late_writes`）があった日。動き続けたときは読まず、起動し直すと前の日のファイルとして読む（4.7）。(2) 前の日のファイルの中で、一度も行が出なかったトラッカーの状態（前の前の日に左右される）。(3) `pushes`（そのアプリの回が push した数。起動し直すと 0 から数える）。(4) 見張りで、L より遅れて届いた行（サーバーの `time_regressions` と、ドライバーの `driver.late`。4.6）。

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
    { "serial": null, "slots": [1], "name": null,
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
| `name` | 設定のニックネーム（9.1）。なければ `null` |
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

- `serial` と `name` は、分かったときだけ。集計にはシリアル番号が入るので、利用者の PC にだけ置く。送る機能（L-4）では、匿名化してから送る。
- 「位置が出ていた割合」は、`findings-2026-11-28.md` の「位置が出ていたフレームの割合」とは数え方が違う（あちらはトラッカー内の毎フレームの記録、こちらは PC のログの状態の変わり目）。突き合わせはまだしていない。

## 9. 設定と保存場所（W-9）

### 9.1 設定のファイル

- 置き場所: `%APPDATA%\OpenUT\config.toml`（`--config` で変えられる）。なくても動く（すべて既定値）。読めない・形が違うときは、既定値で動き、状態の窓とアプリのログに理由を出す（止まらない）。
- 形は TOML（`toml` 1.2.3、`serde` 1.0.229。どちらも MIT OR Apache-2.0）。アプリが書き直すとき（名前の変更、通知の切り替え）は、ファイル全体を書き直す（コメントは残らない。仕方ないものとし、ファイルの先頭にそう書く）。

```toml
# OpenUT の設定。OpenUT が書き直すことがある（コメントは消える）。

# ログのフォルダー。書かなければ自動で探す（9.2）。
# log_dir = 'D:\path\to\logs'

[notify]
steamvr = true
desktop = true

# トラッカーの名前。キーはシリアル番号（状態の窓で名前を付けると書かれる）。
# 台数は決めない。書かなくてよい。
[trackers."<シリアル番号>"]
name = "腰"

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

- **台数とシリアル番号を固定しない**: 状態はログに出てきた番号から作る（5.1）。設定の `trackers` は名前を付けるためだけのもので、書かなくても全台を見張る。シリアル番号を設定に書くのは、利用者が名前を付けたときだけ（利用者の PC にだけ置く）。
- 番号 N はつなぎ直しで変わる（3.2）ので、名前のキーはシリアル番号だけにする（初版の `index:N` のキーはやめた）。シリアル番号が分からない番号には名前を付けられず、`トラッカー{N}` と出す。

### 9.2 ログのフォルダーを探す

順に試し、`ViveTrackerServer_*.txt` があるフォルダーを使う。

1. 設定の `log_dir`、またはコマンドの `--log-dir`。
2. 既知の候補: `%ProgramData%\HTC\ViveSoftware\ViveUltimateTracker\Log`（メモの 1 章。VIVE Hub の 1 つの版で確かめた場所。利用者のフォルダー名を含まない）。`%ProgramData%` は環境変数から作る（変数がなければ、この候補を飛ばす）。候補を足すときも環境変数からの相対で書き、利用者のフォルダー名を設計にもコードにも書かない。ドライバーのファイルは、見つけたフォルダーと同じ場所で探す（4.1）。
3. 見つからなければ「ログが見つからない」（7.4、F4）。

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

- 立て直しの命令（D-13）は M3 で、M2 の命令の一覧（Issue #6）に入ってから。
- レビューでは、M1 の PR に、HID・名前付きパイプ・ドングルを扱うコードや依存が入っていないことを見る。

## 12. 性能（VR の動作を重くしない）

- 見張りは 500 ms ごとに、フォルダーの一覧と 2 つのファイル（サーバー、ドライバー）の大きさを見て、伸びた分だけを読む。1 行の判定は、本文の先頭を数回比べるだけ（4.4）。
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

**オーナーに確かめてもらいたいこと**（PM が `needs-human` にまとめる。どれも 10 分以内）

1. **書いている最中のログを読めるか（U-13）と、書かれる間隔（U-8）。** VIVE Hub を起動してトラッカーを 1 台つなぎ、メモ帳で `%ProgramData%\HTC\ViveSoftware\ViveUltimateTracker\Log` の `ViveTrackerServer_<今日の日付>.txt` を開く。開けるか、開けたら「ファイル → 開く」でもう一度開いたときに行が増えているかを書く。続けて、PowerShell で `1..30 | % { (Get-Item "$env:ProgramData\HTC\ViveSoftware\ViveUltimateTracker\Log\ViveTrackerServer_<今日の日付>.txt").Length; Start-Sleep 1 }` を動かし（トラッカーを動かしながら）、出た 30 個の数をそのまま貼る（大きさが変わる間隔が、まとめて書かれる間隔）。開けなければ、設計を直す（4.2）。
2. **電源オフの予告が、ボタンで電源を切ったときに出るか（U-11。R4 の条件 5 の根拠）。** トラッカーを 3 台つないで 1 分待ってから、1 台ずつ、電源ボタンで電源を切る。それぞれ切った時刻（時計で分と秒）を控え、30 秒あける。終わったら、その日の `ViveTrackerDriver_<今日の日付>.txt` で `Tracker is going to power-off.` の行の時刻と、控えた時刻を並べて書く（ファイルは貼らない。時刻と番号だけ）。3 回とも 10 秒以内に合えば、予告は利用者の操作の印として扱う。合わなければ、R4 の既定を見直す。
3. **ホストの電源の入れ直しで、「Wi-Fi 情報なし」が直るか（R1 の案内の根拠）。** 次に「始まらない」が起きたときに、ドングルのリセットをせず、ホストのトラッカーの電源だけを入れ直し、ほかのトラッカーが使えるようになるまでの時間と、そのときのログを残す。ホストは、サーバーのファイルの最後の `Tracker (N) - switch to HOST mode.` の N（いまの番号）。直らなかったら、どのくらい待ったかを書く。
4. **性能（12 章）。** M1-12 のあと。

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
- カメラを覆うと、ログに喪失が出るまでの遅れ（7.4 の F3）。
