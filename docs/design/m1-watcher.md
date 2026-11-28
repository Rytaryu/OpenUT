# M1 見張り役の設計

VIVE Hub を使ったまま横で動かし、VIVE Hub のトラッカー用サーバーのログを読んで、始まらない・途切れる理由と対処を、その場で知らせる（PRD の 5 章）。最終更新: 2026-11-28（初版。Issue #5）。

## 0. 範囲と前提

- 根拠: `docs/product/PRD.md`（4〜6 章、11 章）、`docs/project/DECISIONS.md`（D-9〜D-17）、`docs/research/findings-2026-11-27.md`、`docs/research/findings-2026-11-28.md`（PR #3）、`overview.md`（2026-11-27 の初版の、ログの行と規則の案）。
- **ログの書式のメモ（Issue #7）が main に入る前に書いた版**。書式について確かめていない点は 4.8 に並べた（U-1〜U-16）。メモが入ったら、PM がまとめて追補を依頼する。そのときに直す場所は、4.4 の表、4.8、6 章のしきい値。
- M1 は実機に命令を送らない（W-7、D-13）。読むのはログのファイルだけ。書くのは、OpenUT 自身の保存場所のファイル、デスクトップの通知、SteamVR の中の表示だけ（11 章）。
- トラッカー内のログの収集（L-3）は、この文書の範囲に入れない。オーナーの PC のつなぎのスクリプトを確かめてから、別に依頼が出る（STATUS の「次の一手」の 6）。
- 画面と通知の**見た目**は UI/UX デザイン担当の範囲（`docs/design/ui/`）。この文書は、何をいつ見せるか、案内の文言の意味、操作の流れまでを決める（7 章）。
- 自分たちで測っていない数値は、そう書く。しきい値はすべて仮の値で、決め方を書く（6.1）。

## 1. 要件との対応

| 要件 | 設計の場所 | 実装計画 | 確かめ方 |
|---|---|---|---|
| W-1 ログを追記分だけ読み続ける（UTF-16、0 時の切り替わり） | 4.1〜4.7 | M1-1、M1-3、M1-4 | 単体テスト（解読、行の先頭、行の種類、追記の追跡）。オーナーの実機の確認（VIVE Hub が書いている最中に読めるか。U-13） |
| W-2 トラッカーごとの状態と、PC の Wi-Fi 情報 | 5 章 | M1-5 | 単体テスト（遷移の表の行ごと） |
| W-3 「Wi-Fi 情報なし」を検出し、電源を入れ直すトラッカーを案内する。全台オフやドングルのリセットを勧めない | 6.2 の R1、7.1 | M1-6 | 単体テスト、合成ログの再生テスト、受け入れの基準のテスト（M1-13） |
| W-4 地図同期が遅い、ホストが位置を出せない、操作と関係のない切断 | 6.2 の R2、R3、R4 | M1-6 | 単体テスト、合成ログの再生テスト |
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
 VIVE Hub のログのフォルダー
        │ （読み取り専用。500 ms ごとに大きさを見る）
        ▼
 ┌─ openut（アプリ）───────────────────────────────────────────┐
 │  追記の読み取り（ファイルの I/O）                           │
 │        │ バイト列                                           │
 │        ▼                                                    │
 │  ┌─ openut-hublog（入出力なし）──────────────┐              │
 │  │ 追記の追跡の判断 → 解読（UTF-16）→ 行     │              │
 │  │ → 行の先頭の解析 → 行の種類 → 出来事     │              │
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
- 規則と集計は、**ログの時刻だけ**を使う。壁時計は、見張りのループが「時刻の進め方」（4.6）のために使うだけで、エンジンには `LogTime` として渡す。
- 時間の長さはミリ秒の `i64` で持つ。

### 3.2 トラッカーの呼び名（`openut_core::tracker`）

```rust
/// The tracker number used by the source of events.
/// M1: the number N in "(N)" / "(index: N)" in the VIVE Hub log.
/// M3: the slot number of the runtime.
pub struct TrackerKey(pub u16);
/// A serial number, when the source tells it. Never written to the repository.
pub struct Serial(pub String);
```

- 台数を決めない（W-9）。状態は `TrackerKey` をキーにした表で持ち、初めて出てきた番号で行を足す。
- 利用者に見せる名前は、設定のニックネーム（9.1。シリアル番号が分かればシリアル番号で、分からなければ番号で引く）。なければ `トラッカー{N}`。
- 番号 N が、つなぎ直しや再起動で変わるか、シリアル番号との対応がログに出るかは未確認（U-6）。

### 3.3 ログの 1 行（`openut_hublog`）

```rust
/// A line split into its header and body (4.3).
pub enum ParsedLine<'a> {
    Entry(Entry<'a>),
    /// No header could be read: a continuation or a broken line.
    NoHeader(&'a str),
}
pub struct Entry<'a> {
    pub at: LogTime,
    pub offset: UtcOffset,
    pub level: &'a str,
    pub func: &'a str,
    pub thread: Option<&'a str>,
    pub body: &'a str,
}
/// What a body says (4.4).
pub enum LineKind { TrackerAdded, TrackerRemoved, HostMode, HostWifiQuery, HostWifiGot,
    WifiAssign, WifiNotYet, ClientLink, MapState, MapStatus, Pose,
    OetLost, OetReloc, OetOverspeed, Other }
pub struct Classified { pub kind: LineKind, pub tracker: Option<TrackerKey>, pub value: Option<i64> }
```

行の種類が分かったのに、番号や値が読めなかったときは `Classified` を作らず、「読めなかった行」として数える（4.7）。

### 3.4 出来事（`openut_core::event`）

状態・規則・集計の入力。**ログの行の形に依存しない**（R-9。M3 では、ランタイムが同じ出来事を出す）。

```rust
pub struct Timed { pub at: LogTime, pub offset: UtcOffset, pub event: Event }
pub enum Event {
    ServerStarted,                                            // U-10: line not known yet
    TrackerAdded { tracker: TrackerKey },
    TrackerRemoved { tracker: TrackerKey },
    SerialKnown { tracker: TrackerKey, serial: Serial },      // U-6: line not known yet
    HostAssigned { tracker: TrackerKey },                     // PC told the tracker to be the host
    HostWifiQueried { host: Option<TrackerKey> },             // PC asked the host for its Wi-Fi info
    HostWifiObtained { host: Option<TrackerKey> },            // PC got the host's Wi-Fi info
    WifiRequestUnanswered { tracker: Option<TrackerKey> },    // a client asked, PC had nothing
    WifiAssigned { tracker: TrackerKey },                     // PC gave the info to a client
    ClientLink { tracker: TrackerKey, connected: bool },      // client joined / left the host's Wi-Fi
    Map { tracker: Option<TrackerKey>, state: MapEvent },
    Pose { tracker: Option<TrackerKey>, status: PoseStatus },
    Oet { tracker: Option<TrackerKey>, kind: OetKind },
    PcCommand { tracker: TrackerKey, kind: String },          // other PC -> tracker commands (U-16)
}
pub enum MapEvent { Synced, ReuseOk, Other(i64) }             // "map state: 2", "map status: 6"
pub enum PoseStatus { Ok, Lost, RecentlyLost, Other(i64) }    // 2, 3, 4
pub enum OetKind { LostTracking, RelocBack, Overspeed }
```

- `HostAssigned` と `WifiAssigned` は、PC からトラッカーへの命令でもある。R4 の「直前に命令があったか」は、この 2 つと `PcCommand` を見る。
- `tracker` が `Option` のものは、ログの行にトラッカーの番号が入るか確かめていないもの（U-5）。入っていなければ `None` で、5 章の「番号のない出来事」の扱いになる。

### 3.5 状態の写し（`openut_core::state`）

```rust
pub enum Phase { Disconnected, Connected, WaitingWifi, MapSyncing, Relocalizing, Tracking }
pub enum Role { Unknown, Host, Client }
pub struct TrackerView {
    pub key: TrackerKey, pub serial: Option<Serial>, pub role: Role,
    pub phase: Phase, pub since: LogTime,
    pub tracked_since_connect: bool,   // reached Tracking at least once since TrackerAdded
    pub inferred: bool,                // first seen without TrackerAdded (started mid-way)
}
pub enum HostWifi {
    Unknown,                                                    // no evidence yet
    Missing { host: Option<TrackerKey>, since: LogTime, queried: bool },
    Held { host: Option<TrackerKey>, since: LogTime },
}
pub struct Snapshot { pub trackers: Vec<TrackerView>, pub host_wifi: HostWifi, pub at: LogTime }
```

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
    /// Fire every timer whose deadline is <= t (in deadline order), then set now = t.
    pub fn advance_to(&mut self, t: LogTime);
    /// advance_to(ev.at), then update the state, the rules and the summary.
    pub fn apply(&mut self, ev: Timed);
    pub fn note_line(&mut self, at: Option<LogTime>, quality: LineQuality); // Other / Unparsed / NoHeader
    pub fn set_catching_up(&mut self, on: bool);                            // 4.5
    pub fn next_deadline(&self) -> Option<LogTime>;
    pub fn drain(&mut self) -> Vec<Output>;   // Detection, Notice, DayClosed(Summary)
    pub fn snapshot(&self) -> Snapshot;
}
```

- `EngineConfig` は、しきい値（6 章の表の既定値）と配信の決まりの値を持つ。設定ファイル（9.1）と再生テスト（10.2）から差し替えられる。
- 時刻が戻る出来事（`ev.at` < いまの時刻）は、いまの時刻に起きたものとして扱い、数える（4.6）。

## 4. ログの読み取り（W-1）

### 4.1 ファイルの選び方と 0 時の切り替わり

- 対象は、ログのフォルダー（9.2）の直下の `ViveTrackerServer_YYYYMMDD.txt`（大文字と小文字を区別しない）。名前の日付が最も新しいものを「いまのファイル」にする。名前が形に合わないファイルは無視する。
- 500 ms ごとに、フォルダーの一覧と、いまのファイルの大きさを見る（OS のファイル変更の通知は使わない。追記の検出だけなら定期の確認で足り、通知の取りこぼしを考えなくてよいため）。
- **0 時の切り替わり**: いまのファイルより新しい日付のファイルが現れたら、いまのファイルを最後まで読み、行の途中で終わっていた残りを 1 行として流してから、新しいファイルの先頭に移る。移ったあと 30 秒のあいだに古いファイルが伸びたら、その分は読まずに「切り替わったあとの書き足し」として数え、アプリのログに残す（VIVE Hub が切り替わりのあとに古いファイルに書くかは未確認。U-9）。
- いまのファイルが縮んだ（大きさが読んだ位置より小さい）ら、作り直されたとみなし、先頭から読み直す（解読の途中の状態も捨てる）。
- いまのファイルが消えたら、残っているうちで最も新しいものに移る。1 つもなければ「ログが見つからない」（7.4 の見張り役の状態）にして、見つかるまで待つ。
- 判断（どのファイルを、どこから読むか）は `openut_hublog::tail` に入出力なしで書き、ファイルの一覧と読み取りは、アプリが渡すトレイト（例: `LogSource { list(), read_at(name, offset, max) }`）を通す。テストでは、メモリ上の偽物を渡す。

### 4.2 追記分だけ読む（UTF-16）

- ファイルは読み取り専用で開く（`std::fs::OpenOptions` の `read(true)` だけ）。読むたびに開いて、前回の位置から最後まで読み、閉じる。開いたままにしないので、VIVE Hub がファイルを消したり名前を変えたりするのを妨げない。Windows では、Rust の標準ライブラリの既定の共有モードが `FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE`（Rust 1.100.0 の `library/std/src/os/windows/fs.rs` の `share_mode` の説明で確認）なので、書いている VIVE Hub を妨げない。VIVE Hub 側が読み取りの共有を許しているかは未確認（U-13）。
- 1 回に読む量は 4 MiB まで。それより多く残っていれば、待たずに続けて読む（起動時の追いつき）。
- **解読**（`openut_hublog::decode`。入出力なし）は、バイト列を少しずつ受け取って行を返す。
  - 文字コードは、ファイルの先頭で決める: `FF FE` なら UTF-16LE（BOM は捨てる）、`FE FF` なら UTF-16BE、`EF BB BF` なら UTF-8、BOM がなければ、先頭の 2 バイト目と 4 バイト目が 0 なら UTF-16LE、そうでなければ UTF-8。実物は UTF-16（PRD の W-1）。UTF-8 も読めるのは、再生テストの合成ログを読みやすく書くため（10.1）。
  - UTF-16 で、読んだバイト数が奇数なら、最後の 1 バイトを次回に持ち越す。サロゲートの片方で終わったら、それも持ち越す。対にならないサロゲートと、UTF-8 の壊れた並びは、置き換え文字（U+FFFD）にする（止まらない）。解読は標準ライブラリ（`char::decode_utf16`、`String::from_utf8_lossy` の考え方）で書き、クレートを足さない。
  - 行の終わりは `\n`。直前の `\r` は捨てる（改行が CRLF かは未確認。U-2）。最後の行の終わりがなければ、次回まで持ち越す。
  - 1 行が 64 KiB を超えたら、そこで切って 1 行として流し、「長すぎる行」として数える。

### 4.3 行の先頭

初版の調べ（`overview.md` の 2026-11-27 版）による形:

```
[YYYY-MM-DD hh:mm:ss.mmm +9 0x.... LEVEL 関数名] [スレッド番号] 本文
```

解析（`openut_hublog::header`）は寛容にする:

1. `[` で始まり、日付 `YYYY-MM-DD`、空白、時刻 `hh:mm:ss.mmm`（ミリ秒は 1〜6 桁を受け付け、ミリ秒に丸める）、空白、ずれ（`+` か `-` と、時の 1〜2 桁、任意で `:mm`）が読めれば `Entry`。どれかが読めなければ `NoHeader`。
2. そのあと、最初の `]` までを空白で区切り、1 つ目を識別子（`0x....`。保存しない）、2 つ目を `level`、残り（空白を含んでもよい）を `func` とする。足りなければ空の文字列。
3. `]` のあとに空白と `[...]` があれば、その中を `thread`。
4. 残りの先頭の空白を除いたものを `body`。

- `+9` を UTC からのずれ（時）と読むのは推定（U-3）。読み違えていても、見張りの時刻の進め方（4.6）は壁時計とのずれを測って補うので、検出の時刻は狂わない。集計の日付は、ファイル名の日付を正とする（8 章）。
- 本文が複数行にわたる行があるか（`NoHeader` の行が続くか）は未確認（U-4）。`NoHeader` の行は、規則には渡さず数えるだけにする。

### 4.4 行の種類と出来事

本文の判定（`openut_hublog::record`）は、下の表の決まった語句を本文に含むかで行う（前後に別の語句がついていてもよい）。トラッカーの番号は、本文の中の `(N)`、`(index: N)`、`Tracker (N)` の形から取る。表にない本文は `Other`（ほとんどの行はこれ。数えるだけ）。

| 種類 | 本文の語句（初版の調べ。#7 で確かめる） | 取り出すもの | 出来事 |
|---|---|---|---|
| TrackerAdded | `Add new tracker: (N)` | N | `TrackerAdded` |
| TrackerRemoved | `Remove tracker (N)` | N | `TrackerRemoved` |
| HostMode | `switch to HOST mode` | N（`Tracker (N)`） | `HostAssigned` |
| HostWifiQuery | `Query wifi` | N（あれば） | `HostWifiQueried` |
| HostWifiGot | `Got host wifi info` | N（あれば） | `HostWifiObtained` |
| WifiAssign | `Assign wifi info to client tracker` | N（`index: N`） | `WifiAssigned` |
| WifiNotYet | `Wifi info is not yet available` | N（あれば） | `WifiRequestUnanswered` |
| ClientLink | `is connected to host:` | N（`index: N`）、値（0 か 1） | `ClientLink` |
| MapState | `map state:` | 値（2 なら同期済み）、N（あれば） | `Map`（2 → `Synced`、ほか → `Other`） |
| MapStatus | `map status:` | 値（6 なら再利用に成功）、N（あれば） | `Map`（6 → `ReuseOk`、ほか → `Other`） |
| Pose | `Pose data - status:` | 値、N（あれば） | `Pose`（2 → `Ok`、3 → `Lost`、4 → `RecentlyLost`、ほか → `Other`） |
| OetLost | `OET LOST TRACKING` | N（あれば） | `Oet(LostTracking)` |
| OetReloc | `OET reloc back` | N（あれば） | `Oet(RelocBack)` |
| OetOverspeed | `OET OVERSPEED` | N（あれば） | `Oet(Overspeed)` |

- 値の意味は、初版の調べ（解析ライブラリ pyvut の定数の名前 `POSE_OK`、`MAP_REUSE_OK` などと合う、という記述）による。pyvut（<https://github.com/rocketmark/pyvut>）にはライセンスの表示がないので、定数の表は写さず、意味だけを使う。どのファイルの定数かは、この版では確かめていない。
- 語句は大文字と小文字を区別して比べる（実物の綴りを #7 で確かめてから、区別するかを決め直す）。
- `ServerStarted`、`SerialKnown`、`PcCommand` を出す行は、まだ分からない（U-6、U-10、U-16）。#7 のあとで表に足す。
- 1 行の判定は、決まった語句の部分一致を数回するだけにする（正規表現のクレートを足さない。行数が多くても軽いように）。

### 4.5 途中から読み始めたとき（追いつき）

- 起動したら、いまのファイルを**先頭から**読む。その日のうちの出来事から状態を組み立て、その日の集計（8 章）も最初から数えるため。
- 起動した時点のファイルの終わりまでを読み終えるまでを「追いつき」とし、エンジンに `set_catching_up(true)` を渡す。追いつきのあいだも、状態・規則・集計はふだんどおり進めるが、配信（7.1）は通知を出さない。
- 追いつきが終わったら、まず見張りの時刻（4.6）まで進め、**その時点で続いている**検出（`Raised` のあと、`Resolved` も `Ended` も出ていないもの）だけを、ふつうの配信の決まりで出す。終わってしまった検出は出さない（集計には数える）。
- 前の日のファイルは読まない。0 時をまたいで使っていて、0 時のあとに起動した場合、それより前の出来事は分からない。その場合、`TrackerAdded` なしで出てきた番号は、行の意味から状態を推し量って足し（`inferred`）、PC の Wi-Fi 情報は `Unknown` から始める（5 章）。

### 4.6 まとめて書かれることと、時刻の進め方

VIVE Hub は数秒おきにまとめてファイルに書く（PRD、`overview.md` の初版。間隔は測っていない。U-8）。そのため、検出は最大でその間隔だけ遅れる。

- エンジンの時刻は、出来事の時刻で進む（3.7）。規則の「何秒たっても〜しない」は、エンジンが期限を持ち、`advance_to` で期限を過ぎたものから順に判定する。
- ログが静かなあいだも期限を判定できるよう、見張りのループは、行がなくても 500 ms ごとに `advance_to(見張りの時刻)` を呼ぶ。

  見張りの時刻 = max(最後に読んだ行の時刻, 今の壁時計 − d − L)

  - d: 壁時計とログの時刻のずれ。読んだまとまりごとに「読んだときの壁時計 − そのまとまりの最後の行の時刻」を測り、直近 5 分の最小値を使う（直近 5 分に測ったものがなければ、最後に測った値）。追いつき（4.5）で読んだ分は古い行なので、d の測定に使わない。d がまだなければ、見張りの時刻は最後に読んだ行の時刻のまま。ずれ（4.3 の `+9`）の読み方が違っても、PC の時計がずれていても、ここで吸収する。
  - L: まとめて書かれる間隔より長い余裕。**仮の値 10 秒**。U-8 の実測（#7 のメモ）で決め直す。短すぎると、まだ書かれていない行が来る前に期限を判定してしまう。長すぎると、期限の検出がその分遅れる。
  - 読んだ行がまだ 1 行もなければ、時刻を進めない。
- 時刻が戻る行（L より遅れて届いた行、PC の時計の変更）は、いまの時刻に起きたものとして扱い、「時刻が戻った行」として数える。
- 再生テストでは壁時計を使わない（10.2）。

### 4.7 想定と違う行

止まらずに読み続ける（受け入れの基準）。どの段でも `panic` しない。

| 起きること | 扱い | 数える名前 |
|---|---|---|
| 行の先頭が読めない | 規則に渡さない | `no_header` |
| 種類は分かったが、番号や値が読めない | 規則に渡さない。最初の 20 件まで、アプリのログに本文を残す | `unparsed` |
| 表にない本文 | 規則に渡さない | `other` |
| 対にならないサロゲート、壊れた UTF-8 | 置き換え文字にして続ける | `replaced_chars` |
| 64 KiB を超える行 | 切って続ける | `long_lines` |
| 時刻が戻る行 | いまの時刻として扱う | `time_regressions` |
| 切り替わったあとの古いファイルの書き足し | 読まない | `late_writes` |

- 数は、日ごとの集計（8 章）の `source` に書く。`unparsed` が増えたら、ログの書式が変わった印なので、状態の窓に「ログの形が想定と違う行が N 行」と出す（案内ではなく、見張り役の状態として。7.4）。
- アプリのログに残す本文には、シリアル番号などが入りうる。アプリのログは利用者の PC にだけ置き、送る機能（L-4）では匿名化を通す。

### 4.8 ログの書式で確かめていない点

#7 のメモが入る前の版なので、次を確かめていない。#7 のあとの追補で、表と決まりを直す。

| 番号 | 確かめていないこと | 影響する場所 |
|---|---|---|
| U-1 | 文字コードが UTF-16LE で、BOM があるか | 4.2 |
| U-2 | 改行が CRLF か | 4.2 |
| U-3 | 行の先頭の正確な形（`+9` が UTC からのずれか、`0x....` の意味、LEVEL の種類、関数名に空白が入るか、スレッド番号の括弧の形） | 4.3 |
| U-4 | 本文が複数行にわたる行があるか | 4.3、4.7 |
| U-5 | 4.4 の各行の本文の正確な綴りと、トラッカーの番号が入るか（特に `map state`、`map status`、`Pose data`、`OET ...`、`Wifi info is not yet available`、`Got host wifi info`、`Query wifi`） | 4.4、5 章 |
| U-6 | 番号 N の意味（0 始まりか、ドングルのスロットか、つなぎ直しで変わるか）と、番号とシリアル番号の対応が出る行 | 3.2、9.1 |
| U-7 | `Pose data` の行が、状態が変わったときだけ出るか、定期的に出るか | 12 章（行数の見積もり）。集計は、どちらでも同じ結果になるように、状態の変わり目から時間を数える |
| U-8 | まとめて書かれる間隔 | 4.6 の L |
| U-9 | 0 時の切り替わりの正確な動き（0 時ちょうどに新しいファイルを作るか、最初の行で作るか、古いファイルに書き足すことがあるか） | 4.1 |
| U-10 | サーバーの開始と終了を示す行 | 5 章（`ServerStarted`）、R4 の除外 |
| U-11 | 利用者がトラッカーの電源を切ったときに出る行 | R4 の除外 |
| U-12 | ドングルの抜き差し・リセットのときに出る行 | R4 の除外 |
| U-13 | VIVE Hub が書いている最中のファイルを、ほかのプログラムが読めるか（共有モード） | 4.2。読めなければ設計の作り直しが要る |
| U-14 | ログのフォルダーの場所（自動検出の候補） | 9.2 |
| U-15 | 1 日あたりの行数とファイルの大きさ | 12 章 |
| U-16 | PC からトラッカーへの命令にあたる行の一覧 | R4 |

U-13 は、#7 のメモでは分からない可能性がある（書いている最中に読めるかは、実際に試すしかない）。オーナーに試してもらう手順の案は 14 章。

## 5. 状態の遷移（W-2）

状態（`openut_core::state`）は、出来事を受けて表を更新するだけで、時刻を自分で進めない。

### 5.1 トラッカー

行は `TrackerKey` ごと。最初に出てきた出来事で行を足す（台数を決めない）。

| 出来事 | 前の状態 | 次の状態 | ほかに変えること |
|---|---|---|---|
| `TrackerAdded(N)` | どれでも | 接続 | 役割を `Unknown`、`tracked_since_connect = false`、`inferred = false` |
| `TrackerRemoved(N)` | どれでも | 未接続 | 役割を `Unknown` |
| `HostAssigned(N)` | 接続、Wi-Fi 待ち | 地図同期中 | N の役割を `Host`。ほかに `Host` だったトラッカーがあれば、その役割を `Unknown` にする。ほかの「接続」のトラッカーを Wi-Fi 待ちにする（役割は変えない） |
| `HostAssigned(N)` | 地図同期中、位置取り直し中、追跡中 | 変えない | 役割の変え方は上の行と同じ。すでに動いているトラッカーへの 2 度目の命令（不具合の始まり）で、状態を巻き戻さないため |
| `WifiAssigned(N)` | 接続、Wi-Fi 待ち | Wi-Fi 待ち | 役割を `Client` |
| `ClientLink(N, true)` | 接続、Wi-Fi 待ち | 地図同期中 | 役割を `Client` |
| `ClientLink(N, false)` | 地図同期中 | Wi-Fi 待ち | |
| `ClientLink(N, false)` | 位置取り直し中、追跡中 | 変えない | 集計に数える（ホストの Wi-Fi から外れたクライアントが位置を出し続けるかは未確認なので、位置の行で判断する） |
| `Map(N, Synced か ReuseOk)` | 接続、Wi-Fi 待ち、地図同期中 | 位置取り直し中 | |
| `Pose(N, Ok)`、`Oet(N, RelocBack)` | 未接続以外 | 追跡中 | `tracked_since_connect = true` |
| `Pose(N, Lost か RecentlyLost)`、`Oet(N, LostTracking)` | 追跡中、地図同期中 | 位置取り直し中 | |
| `Pose(N, Other)`、`Map(N, Other)`、`Oet(N, Overspeed)` | — | 変えない | 集計に数える |
| `ServerStarted` | どれでも | 全台を未接続 | PC の Wi-Fi 情報を `Missing { host: None }` |

- 表にない組み合わせ（例: 未接続のトラッカーの `Pose`）は、状態を変えずに「順序の合わない出来事」として数える。ただし、`TrackerAdded` を一度も見ていない番号（途中から読み始めた）は例外で、その出来事から推し量った状態で行を足し、`inferred = true` にする（`Pose(Ok)` なら追跡中、`ClientLink(true)` なら地図同期中、など。推し量れない出来事なら「接続」）。
- 同じ状態への遷移は、何もしない（`since` を変えない）。`Pose` と `OET` の両方の行が出ても、二重に数えない。
- **番号のない出来事**（`tracker: None`。U-5 で番号が入らないと分かった種類）: 当てはまりうるトラッカー（`Map` なら地図同期中のもの、`Pose` と `Oet` なら未接続以外のもの）がちょうど 1 台なら、その台の出来事として扱う。2 台以上か 0 台なら、状態を変えずに数える。#7 で、行の先頭のスレッド番号などからトラッカーが分かると判明したら、`openut_hublog::record` で番号を補う形に直す。
- ホストの地図: ホストが自分の地図を読み込んだ印（`map status: 6` がホストの行か）は未確認。ホストは `HostAssigned` で地図同期中になり、`Map` か `Pose(Ok)` で先へ進む。

### 5.2 PC 側の Wi-Fi 情報

VIVE Hub のサーバーが、ホストの Wi-Fi 情報を持っているか。PC が実際に持っているかではなく、**ログから読み取れること**を表す。

| 出来事 | 次の状態 |
|---|---|
| （起動時） | `Unknown` |
| `ServerStarted` | `Missing { host: None, queried: false }` |
| `HostAssigned(N)` | `Missing { host: Some(N), since: いま, queried: false }`（前に `Held` でも。2 度目の命令で持たない状態になるのが不具合の始まり。`findings-2026-11-27.md`） |
| `HostWifiQueried` | `Missing` なら `queried = true`。ほかは変えない |
| `HostWifiObtained` | `Held { host: 最後に HostAssigned した番号, since: いま }` |
| `WifiAssigned` | `Held` でなければ `Held` にする（渡せたのだから持っている）。「順序の合わない出来事」として数える |
| `WifiRequestUnanswered` | 変えない（R1 が数える）。`Held` のときに出たら「順序の合わない出来事」として数える |
| `TrackerRemoved`（ホストが外れた） | 変えない（次のホストへの命令で `Missing` になる） |

## 6. 検出の規則（W-3、W-4）

### 6.1 共通の決まり

- 規則は、状態と出来事と時刻から `Detection` を出す（入出力なし）。1 回の「条件が成り立ってから、なくなるまで」を**エピソード**と呼び、番号をつける。エピソードは、`Raised`（始まり）→ 任意で `Escalated`（長引いた）→ `Resolved`（直った）か `Ended`（直ったとは言えないが、条件がなくなった）の順に出る。
- エピソードのあいだ、同じ規則・同じトラッカーの `Raised` は出さない。案内を何回出すか（`push`）は、7.1 の配信の決まりが決める。
- 規則の判定に使うのは、ログの時刻だけ。
- **しきい値はすべて仮の値**。決め方:
  1. 匿名化した 2026-11-22〜27 のログ（L-2。PM が `tests/` に入れる）を、`openut replay` で、しきい値を変えながら流す（`--set` で上書き。実装計画の M1-9）。
  2. R1 は、受け入れの基準（4 回だけ。10.4）を満たす範囲を探し、その範囲の真ん中寄りの値にする。R2〜R5 は、`findings-2026-11-27.md` の数字（喪失 234 回、1 分を超える喪失 19 回、地図の受け取り 1〜12 秒と 40〜550 秒、操作と関係のない切断 1 日 1〜3 回）と、出る案内の数を見比べて、1 晩に出る案内が数回に収まる値にする。
  3. 決めた値と、そのときの検出の数を、実装の PR の説明に書く。値を変えるのは設計の変更なので、PM に報告する（設計の追補で、この表を直す）。
- しきい値の名前（`r1.count` など）は、設定ファイル（9.1）と `--set` で共通。

### 6.2 規則

#### R1 Wi-Fi 情報なし（W-3）

- **根拠**: `findings-2026-11-27.md` の「始まらない問題」。4 回とも、`switch to HOST mode` が 2 度出て、2 度目のあとに `Query wifi` と `Got host wifi info` が出ず、以後 `Wifi info is not yet available.` が約 0.55 秒おきに続き、後から来たトラッカーに `Assign wifi info` が一度も出なかった。
- **数え始め**: 最後の `HostAssigned`、`HostWifiObtained`、`WifiAssigned`、`ServerStarted` のうち、最も新しいもの。そこから先の `WifiRequestUnanswered` を数える。
- **条件**: PC の Wi-Fi 情報が `Held` でなく、数え始めのあとの `WifiRequestUnanswered` が `r1.count` 回以上で、その 1 回目から `r1.span_s` 秒以上たった `WifiRequestUnanswered` が来たとき、`Raised`（時刻はその行の時刻）。
  - 仮の値: `r1.count = 3`、`r1.span_s = 15`。ふつうの開始でも、ホストが Wi-Fi の親機を立てるまでのあいだに、クライアントが尋ねて断られることがありうる（測っていない）。`span_s` は、それを不具合と見なさないための余裕。決め方は 6.1。
  - `Query wifi` が出たかどうか（`queried`）は条件に入れない（綴りを確かめていないため）。`Detection` の詳細として残し、#7 と再生テストのあとで、条件に足すかを決める。
- **案内するトラッカー**: 最後に `HostAssigned` された番号（ホスト）。分からなければ（`Unknown` から始まったとき）「ホストのトラッカー」。
- **長引いたとき**: `Raised` から `r1.escalate_s`（仮 180 秒）たっても続いていたら、`Escalated` を 1 回だけ出す。
- **直った**: `HostWifiObtained` か `WifiAssigned` が来たら `Resolved`。
- **終わった**: 未接続以外のトラッカーがなくなったとき、または `WifiRequestUnanswered` が `r1.quiet_s`（仮 60 秒）来なかったとき、`Ended`。
- エピソードのあいだに次の `HostAssigned` が来ても、エピソードを続ける（ホストの電源を入れ直すと、まず `HostAssigned` が来て、そのあとに `HostWifiObtained` が来ると見込むため。見込みは未確認）。
- **電源を入れ直すトラッカーがホストであること**の効果は、まだ確かめていない（`findings-2026-11-27.md` の「次に確かめること」の「ホストの入れ直しで不具合が直るか」）。案内の文言はこの見込みに基づく。オーナーに試してもらう手順の案は 14 章。

#### R2 地図同期が遅い（W-4）

- **根拠**: クライアントが地図を受け取るまで 1〜12 秒の回と 40〜550 秒の回がある（`findings-2026-11-27.md`）。
- **条件**: クライアント N が `ClientLink(N, true)` で地図同期中になってから `r2.wait_s`（仮 30 秒）たっても地図同期中のままなら、`Raised`。
- **直った**: N が位置取り直し中か追跡中になったら `Resolved`。**終わった**: N が未接続か Wi-Fi 待ちになったら `Ended`。
- 案内の「ホストの近くに置く」は、地図がトラッカー同士の Wi-Fi で渡ることからの推定で、効果は確かめていない。

#### R3 位置合わせが終わらない（W-4 の「ホストが位置を出せない」を含む）

- **根拠**: ホストが位置を出すまで 5.5〜322 秒（`findings-2026-11-27.md`）。SLAM の開始から最初の位置まで 21〜62 秒。静止したまま起動すると 7 分 45 秒失敗し続けた回がある（`findings-2026-11-28.md`）。
- **対象**: すべてのトラッカー。ホストは W-4 のとおり。クライアントにも同じ規則を当てる（位置合わせの待ちはどのトラッカーにも起き、案内も同じため）。
- **条件**: 接続してから一度も追跡中になっていない（`tracked_since_connect = false`）トラッカーが、「位置合わせを始めた時刻」から `r3.wait_s`（仮 60 秒）たっても追跡中にならなければ、`Raised`。位置合わせを始めた時刻は、ホストなら `HostAssigned` の時刻、クライアントなら位置取り直し中になった時刻。
  - 初版の案の 30 秒は、最初の位置まで 21〜62 秒かかることと合わない（ほとんど毎回出る）ので、60 秒にした。
- **直った**: 追跡中になったら `Resolved`。**終わった**: 未接続になったら `Ended`。

#### R4 操作と関係のない切断（W-4）

- **根拠**: 操作と関係のない切断は 1 日 1〜3 回（`findings-2026-11-27.md`）。SLAM のクラッシュで接続ごと切れた例（11/23 22:38。`findings-2026-11-28.md`）。
- **条件**: `TrackerRemoved(N)` が来て、次のすべてを満たすとき、`Raised`。
  1. 直前 `r4.cmd_window_s`（仮 12 秒。`findings-2026-11-27.md` で喪失と命令の関係を見たときの幅）以内に、PC から N への命令（`HostAssigned`、`WifiAssigned`、`PcCommand`）がない。
  2. N が `TrackerAdded` から `r4.min_connected_s`（仮 30 秒）以上つながっていた（開始のときのつなぎ直しを除く）。
  3. ほかのトラッカーの `TrackerRemoved` が前後 `r4.burst_s`（仮 2 秒）以内にない（全台が同時に外れるのは、ドングルの抜き差しやリセット、サーバーの終了と見なす。U-12）。
  4. 前後 5 秒以内に `ServerStarted` がない（U-10）。
- 3 と 4 で切断のあとの数秒を見るため、`Raised` は切断から 5 秒（`r4.burst_s` と 4 の幅の大きいほう）たった時点で、条件を確かめてから出す。
- 利用者が電源を切ったときの行が分かれば（U-11）、それも除く。それまでは、電源を切ったときにも案内が出うる。
- **直った**: `r4.back_s`（仮 600 秒）以内に `TrackerAdded(N)` が来たら `Resolved`（配信は状態の窓だけ。7.1）。来なければ `Ended`。
- R4 の条件を満たした切断は、案内を出したかどうかにかかわらず、集計の「操作と関係のない切断」に数える。

#### R5 使用中の長い喪失（足した規則）

- **足した理由**: 使っている途中で数分戻らない喪失がある（PRD の 1 章の「長い喪失」。1 分を超える喪失は 6 日で 19 回、11/27 00:18 に約 7 分。`findings-2026-11-27.md`）。W-4 の 3 つには入らないが、利用者が気づかないまま続くと困るので足す。要件に足すかは PM に判断を仰ぐ（14 章）。PM が要らないと決めたら、既定で切る（設定 `rules.r5.enabled`）。
- **条件**: 接続してから一度は追跡中になった（`tracked_since_connect = true`）トラッカーが、追跡中でなくなってから `r5.wait_s`（仮 60 秒）たっても追跡中に戻らなければ、`Raised`。
- **直った**: 追跡中に戻ったら `Resolved`（配信は状態の窓だけ）。**終わった**: 未接続になったら `Ended`。

### 6.3 規則にしないもの

| もの | 扱い | 理由 |
|---|---|---|
| `OET OVERSPEED!` | 集計に数えるだけ | 何をすればよいかの根拠がない。走り出し（R-6）の検知は、姿勢を直接読める M3 で行う |
| 短い喪失（`r5.wait_s` より短い） | 集計に数えるだけ | 6 日で 234 回あり、案内にすると多すぎる |
| `Query wifi` が出たのに `Got host wifi info` が来ない | 数えるだけ（`Detection` にはしない） | 実例を見ていない。#7 と再生テストで実例が見つかれば、規則にするかを決める |
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
| R5 | push | — | 欄から消すだけ | 欄から消すだけ | R2 と同じ |

全体の決まり:

- **間隔**: push は、前の push から 10 秒（`notice.min_gap_s`）以上空ける。そのあいだに来た push は待たせ、まとめて 1 つにする（優先度の最も高いものを出し、見出しに「ほか N 件」を足す）。優先度は R1 > R4 > R3 > R2 > R5。
- **追いつきのあいだ**（4.5）: push しない。終わった時点で続いているエピソードを、優先度の順に、上の間隔の決まりで出す。
- **直ったら待たせている案内を捨てる**: 待たせている push のエピソードが、出す前に `Resolved` か `Ended` になったら、出さない。
- 配信の値（`notice.cooldown_s`、`notice.min_gap_s`）も仮の値。再生テストで、1 晩に出る push の数を見て決める（6.1）。

文言（意味。見た目と言い回しの調整は UI/UX デザイン担当。意味を変えるときは設計の変更）。`{T}` は対象のトラッカーの名前、`{H}` はホストの名前（3.2）。オーナーの方針（`roles/ux.md`）に合わせ、見出し（状態）と、いま何をすればよいかの 1 行だけにする。

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
| ログの形が想定と違う | 見張り中のまま、その日の `unparsed` が 100 行（仮）を超えた（4.7） |

**トレイのアイコン**

- 3 つの見え方: ふつう（見張り中・待機中で、案内の欄が空）、案内あり（案内の欄に 1 件以上）、見えていない（ログが見つからない・読めない・止まっている）。
- 指を置いたときの 1 行: 「追跡中 2/3」（追跡中の台数／未接続以外の台数）、「案内 1 件」、「ログが見つかりません」など、いまの見え方の理由。
- メニュー: 「状態を開く」「終了」。左クリックでも状態の窓を開く。

**状態の窓**（小さな窓。閉じてもアプリは終わらない。トレイから開く）

上から:

1. 見張り役の状態の 1 行（例: 「見張り中（11/28 のログ）」）。「ログが見つからない」ときは「フォルダーを選ぶ」ボタン。
2. 案内の欄: 続いているエピソードの見出しと 1 行。新しいものが上。なければ欄ごと出さない。
3. トラッカーの一覧: 名前、ホストの印、状態（W-2 の 6 つの名前）、その状態になってからの時間。行の並びは番号の順。
4. PC の Wi-Fi 情報: 持っている／持っていない／不明。
5. 今日の集計: 位置が出ていた割合、喪失の回数、不具合の回数（8 章）。
6. 設定: 通知（VR の中、デスクトップ）を出すかどうか。

**操作の流れ**

| 流れ | 手順 |
|---|---|
| F1 起動 | 起動 → ログのフォルダーを探す（9.2）→ 見つかれば、窓を開かずにトレイに常駐し、追いつき（4.5）のあと見張り中。見つからなければ、状態の窓を開いて「フォルダーを選ぶ」を出す |
| F2 案内 | push の案内 → VR の中とデスクトップに出る → 状態の窓の案内の欄に残る → 直ったら欄から消える（R1 は「直りました」も push） |
| F3 トラッカーに名前を付ける | 状態の窓の一覧で名前を押して入力する → 設定に保存（9.1）→ 以後の案内と一覧に使う。どの行がどのトラッカーか分からないときは、1 台のカメラを手で覆うと、その行が数秒遅れて「位置取り直し中」に変わるので、それで見分ける（覆うと位置合わせがやり直しになることは、HTC の公式の案内「位置が合わないとき: カメラを手で 5 秒覆ってから離す」からの推定。ログに出るまでの遅れは 4.6） |
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

- 集計（`openut_core::summary`。入出力なし）は、状態の変わり目と検出から、ログの日付ごとに数える。日付は、ログのファイル名の日付（`ViveTrackerServer_YYYYMMDD`）。0 時をまたぐ状態は、新しいファイルの最初の出来事の時刻で区切る（その時点で前の日を閉じ、続いている状態を新しい日に持ち越す）。
- **その日のファイルだけから決まる**ようにする。アプリを起動し直しても、その日のファイルを先頭から読み直すので（4.5）、同じ結果になる。過ぎた日の分は `openut summarize` で作り直せる（M1-9）。
- 書き出しはアプリ（M1-9）: 60 秒ごと、日が替わったとき、終了のときに、一時ファイルに書いてから名前を変える（途中で落ちても壊れたファイルを残さない）。置き場所は 9.3。

形（JSON。`serde_json` 1.0.154、MIT OR Apache-2.0）:

```json
{
  "schema": 1,
  "date": "2026-11-27",
  "final": false,
  "source": { "lines": 0, "other": 0, "unparsed": 0, "no_header": 0, "replaced_chars": 0,
              "long_lines": 0, "time_regressions": 0, "late_writes": 0, "out_of_order": 0 },
  "trackers": [
    { "key": 1, "serial": null, "name": "腰",
      "connected_s": 0.0, "in_use_s": 0.0, "tracking_s": 0.0, "tracking_ratio": null,
      "sessions": 0, "first_pose_s": [], "losses": 0, "long_losses": 0,
      "unexpected_disconnects": 0, "overspeed": 0, "host_count": 0 }
  ],
  "host_starts": 0,
  "faults": { "wifi_info_missing": 0, "unexpected_disconnects": 0 },
  "detections": { "R1": 0, "R2": 0, "R3": 0, "R4": 0, "R5": 0 },
  "pushes": 0
}
```

| 値 | 意味 |
|---|---|
| `final` | 次の日のファイルが現れて閉じた日なら `true` |
| `connected_s` | 未接続以外だった時間 |
| `in_use_s` | 接続してから最初に追跡中になったあと、未接続になるまでの時間（起動直後の待ちを除く） |
| `tracking_s` | 追跡中だった時間 |
| `tracking_ratio` | **位置が出ていた割合** = `tracking_s ÷ in_use_s`。`in_use_s` が 0 なら `null` |
| `sessions` | `TrackerAdded` の回数 |
| `first_pose_s` | 接続（ホストは `HostAssigned`）から最初の追跡中までの秒数。接続のたびに 1 つ |
| `losses` | **喪失の回数** = 追跡中から位置取り直し中への遷移の回数 |
| `long_losses` | そのうち `r5.wait_s` を超えたもの |
| `unexpected_disconnects` | R4 の条件を満たした切断の回数 |
| `host_starts` | `HostAssigned` の回数 |
| `faults` | **不具合の回数**: `wifi_info_missing` は R1 のエピソードの数、`unexpected_disconnects` は R4 の条件を満たした切断の数（全台の合計） |
| `detections` | 規則ごとのエピソードの数 |
| `pushes` | push した案内の数（出しすぎていないかを見るため） |

- `serial` と `name` は、分かったときだけ。集計は利用者の PC にだけ置く。送る機能（L-4）では、匿名化してから送る。
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

# トラッカーの名前。キーはシリアル番号（ログから分かるとき）か "index:N"。
# 台数は決めない。書かなくてよい。
[trackers."index:1"]
name = "腰"

# しきい値（6 章）。書かなければ既定値。上級者向け。
[rules]
# r1.count = 3
# r1.span_s = 15
```

- **台数とシリアル番号を固定しない**: 状態はログに出てきた番号から作る（5.1）。設定の `trackers` は名前を付けるためだけのもので、書かなくても全台を見張る。シリアル番号を設定に書くのは、利用者が名前を付けたときだけ（利用者の PC にだけ置く）。
- 番号 N がつなぎ直しで変わるなら（U-6）、`index:N` の名前は別のトラッカーを指してしまう。シリアル番号がログから分かれば、シリアル番号をキーにする。どちらも分からなければ、名前は「その回だけ」の扱いにする（#7 のあとで決め直す）。

### 9.2 ログのフォルダーを探す

順に試し、`ViveTrackerServer_*.txt` があるフォルダーを使う。

1. 設定の `log_dir`、またはコマンドの `--log-dir`。
2. 既知の候補（#7 のメモで分かる場所。U-14。この版では空）。環境変数（`%LOCALAPPDATA%`、`%ProgramData%` など）からの相対で書き、利用者のフォルダー名を設計にもコードにも書かない。
3. 見つからなければ「ログが見つからない」（7.4、F4）。

- 候補の場所を、深くたどって探し回ることはしない（起動を遅くしないため）。
- 見つけたフォルダーの場所は、アプリのログにだけ書く（利用者のフォルダー名が入るため、集計には書かない）。

### 9.3 OpenUT が書く場所

| もの | 場所 |
|---|---|
| 設定 | `%APPDATA%\OpenUT\config.toml` |
| 日ごとの集計 | `%LOCALAPPDATA%\OpenUT\summary\YYYY-MM-DD.json` |
| アプリ自身のログ | `%LOCALAPPDATA%\OpenUT\logs\openut-YYYYMMDD.log`（UTF-8。14 日より古いものは起動時に消す） |

- 場所は環境変数から作る（クレートを足さない）。`--data-dir` で変えられる（テストで使う）。
- VIVE Hub のフォルダーには何も書かない。

## 10. 再生テスト（W-8）

### 10.1 入力の置き場所と形

```
tests/replay/<ケースの名前>/
  ViveTrackerServer_YYYYMMDD.txt   … 入力のログ。1 つ以上。名前の日付の順に流す
  expect.toml                      … 期待する検出と集計
  README.md                        … 任意。何を確かめるケースか（1〜3 行）
```

- 入力のログの文字コードは、4.2 の決め方で読む。実物と同じ UTF-16LE（BOM つき）でも、読みやすい UTF-8 でもよい。合成のケースは UTF-8 で書いてよい（差分が読める）。UTF-16 の読み取りそのものは、解読の単体テスト（M1-1）と、UTF-16 で保存した合成のケースを 1 つ置いて確かめる。
- 実物のログは、匿名化の道具（L-1）を通したものだけを入れる（PRD の L-2）。トラッカーの名前は道具の置き換え（例: `TRACKER-A`）のまま。
- 最初に置く合成のケース（M1-2）:
  - `synthetic-r1-second-host-command`: 2 台目のトラッカーが登録される直前に 2 度目の `switch to HOST mode` が出て、そのあと `Wifi info is not yet available.` が 0.55 秒おきに 60 秒続く（`findings-2026-11-27.md` の経過を、行の語句は 4.4 の表で合成したもの）。R1 が 1 回。
  - `synthetic-normal-start`: ホストへの命令、`Query wifi`、`Got host wifi info`、クライアントへの `Assign wifi info`、`is connected to host: 1`、`map state: 2`、`Pose data - status: 2` の順のふつうの開始。検出なし。
  - `synthetic-utf16`: 上のどちらかを UTF-16LE（BOM つき、CRLF）で保存したもの。結果が同じこと。
  - 合成のケースの語句は、#7 のメモが入ったら、見本に合わせて直す（M1-3 の追補）。
- 実物のログ（2026-11-22〜27）を入れるケースの名前は `hub-2026-11-22-27`。大きさによっては、レポジトリに入れ方を PM が決める（14 章）。

### 10.2 時刻の進め方

- 入力のファイルを名前の日付の順につなぎ、行の順に流す。壁時計は使わない。
- 出来事ごとに、エンジンの `apply`（中で、その時刻まで `advance_to`）を呼ぶ。最後の行のあと、`advance_to(最後の行の時刻 + flush_s)`（既定 600 秒。`expect.toml` で変えられる）を呼んで、残っている期限を判定する。
- ファイルとファイルのあいだでは、0 時の切り替わり（8 章の日の区切り）を、見張りのときと同じに起こす。
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

`tests/replay/hub-2026-11-22-27/expect.toml`（M1-13。PM が匿名化したログを入れたあと）:

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

- 見張りは 500 ms ごとに、フォルダーの一覧と 1 つのファイルの大きさを見て、伸びた分だけを読む。1 行の判定は、決まった語句の部分一致を数回するだけ（4.4）。
- SteamVR の中の表示は、案内を出すときだけ画像を渡す。毎フレームの描画はしない（7.2）。状態の窓は、隠しているあいだ描かない（7.4）。
- 起動時の追いつきは、1 回 4 MiB ずつ読む（4.2）。1 日分のファイルの大きさは未確認（U-15）。
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

1. **R5（使用中の長い喪失）を既定で出してよいか。** W-4 の 3 つに入らない、足した規則（6.2）。要らなければ既定で切る。
2. **R3 をホスト以外にも当ててよいか。** W-4 は「ホストが位置を出せない」。設計は全台に当てた（6.2）。
3. **実物のログ（2026-11-22〜27）の入れ方。** 匿名化したあとの大きさによっては、レポジトリに直接入れるのが重い。圧縮して入れるか（`.gitignore` に `*.tgz` がある）、一部の時間帯だけ切り出すか（受け入れの基準は 6 日分を流すことなので、切り出すなら基準の読み方も決める）。

**オーナーに確かめてもらいたいこと**（PM が `needs-human` にまとめる。どれも 10 分以内）

1. **書いている最中のログを読めるか（U-13）。** VIVE Hub を起動してトラッカーを 1 台つなぎ、メモ帳で `ViveTrackerServer_<今日の日付>.txt` を開く。開けるか、開けたら「ファイル → 開く」でもう一度開いたときに行が増えているかを書く。開けなければ、設計を直す（4.2）。
2. **ホストの電源の入れ直しで、「Wi-Fi 情報なし」が直るか（R1 の案内の根拠）。** 次に「始まらない」が起きたときに、ドングルのリセットをせず、ホストのトラッカー（最初に電源を入れたものと推定。違ったら書く）の電源だけを入れ直し、ほかのトラッカーが使えるようになるまでの時間と、そのときのログを残す。直らなかったら、どのくらい待ったかを書く。
3. **性能（12 章）。** M1-12 のあと。

**確かめていないこと**（設計に書いた見込み。実装と #7 で確かめる）

- ログの書式（4.8 の U-1〜U-16）。
- `VRApplication_Background` でオーバーレイを作れるか、`openvr_sys` が CI の Windows でビルドできるか（7.2）。
- トーストの送り主の ID の登録の方法、トーストを押したことを受け取れるか（7.3）。
- 日本語以外の Windows にある書体（7.2）。
- egui の窓を、トレイと同じイベントループで、閉じても終わらずに動かせるか（7.4）。
- R1 の `span_s` を 15 秒にしてよいか（ふつうの開始で、断られる問い合わせが何秒続くか）。R2〜R5 のしきい値（6.1 の決め方で決める）。
- カメラを覆うと、ログに喪失が出るまでの遅れ（7.4 の F3）。
