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
| DG-4 読んだ値を時刻つきで記録できる | 記録の形（8 章）。匿名化した書き出し（8.4） |
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
  Anonymizer（純粋。記録 → 匿名化した記録 ＋ 残っていないかの確認）
```

- 再生（記録ファイルからの読み取り）と実機からの読み取りは、同じ `RawReport` を流す。解読から先は、どちらでも同じコードが動く（M3 の再生テストの土台）。
- 時刻は、受け取った報告に付けた単調な時刻（`mono_us`）だけで進める。純粋な部品は時計を読まない。

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

- `0x28` と `0x18` の宛先の 6 バイトのうち、識別子の 5 バイト（4〜8）で見分ける。初めて見た順に A、B、C… と名付ける（記録 1 本の中で固定）。
- S2 が使う「宛先の 2 バイト目の下位 4 ビット = 枠の番号」は、表示に「枠 n（推定）」として添えるだけにする（U-4）。

### 6.3 文字列の知らせ

- 先頭に数字が 1 文字あれば、枠の番号として外してから、先頭の文字で種類を決める（`m2-dongle-protocol.md` の 4.5 の表。U-6）。
- 個人の情報を含む種類（`NADS`、`NASS`、`NASI`、`NAPI`、`NADN`、`NADI`、`C…`、`c…`、`WH…`、`Ws…`、`Wp…`、`Wt…`）は、値を `Redactable` に包む。`WH` は値を持たない（`WifiHostInfo`。ホストの印として使う）。
- 表にないものは `Unknown { prefix }`（先頭 2 文字だけ）。

### 6.4 トラッカーの状態表（`TrackerTable`）

出来事を受けて、トラッカーごとの `TrackerView` を進める。時刻は `RawReport.mono_us` だけを使う。

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
| 最後の受信から **2.0 秒**（仮の値。12 章）何も来ない | `link` = 途切れた |

M1 の状態（PRD の W-2: 未接続、接続、Wi-Fi 待ち、地図同期中、位置取り直し中、追跡中）への対応は、M3 で入力を差し替えるとき（R-9）に決める。見込みは `m2-dongle-protocol.md` の 8 章の表。

## 7. VIVE Hub のサーバーの扱い（DG-3）

### 7.1 見張り（`ServerGuard`）

- 判定は純粋な関数にする: プロセスの名前の一覧を受けて、「動いている」「動いていない」を返す。名前は大文字と小文字を区別せずに `ViveTrackerServer.exe` と比べる（ログのファイルの名前 `ViveTrackerServer_YYYYMMDD.txt` と、S3 の記録による）。名前の一覧は設定で足せるようにする（VIVE Hub の版で名前が変わったとき）。
- プロセスの一覧は `sysinfo` で取る（10 章）。
- `record` は、始める前に確かめ、動いていれば**ドングルを開かずに**断る（終了コード 2）。記録の途中も **2 秒ごと**に確かめ、現れたら、読むのをやめてドングルを閉じ、記録に `note` を書いて終わる（終了コード 2）。

### 7.2 断るときの案内（意味）

> VIVE Hub のトラッカー用サーバー（ViveTrackerServer.exe）が動いています。ドングルは同時に 1 つのプログラムでしか扱えない前提なので、読み取りを始めません。VIVE Hub を終了し、タスク マネージャーで ViveTrackerServer.exe がないことを確かめてから、もう一度実行してください。

### 7.3 前提の確認（`probe-sharing`）

DG-3 の「前提が正しいかも確かめる」のための試し。**ふだんの使い方ではない。**

- サーバーが動いているときだけ動く（動いていなければ、その旨を出して終わる）。
- ドングルを共有で開こうとし、結果（開けた／開けなかった、と OS の誤りの番号）を出す。開けたら、**30 秒だけ**読んで（送らない）、種類ごとの件数と、`0x28` の通し番号の飛びから見積もった取りこぼしを出し、同じ形で記録する。
- 分かること: U-2（共有で開けるか、読めるか）と U-17。開けて読めても、こちらの読み取りが VIVE Hub の受け取りを減らしていないかは、この道具だけでは分からない。同じ 30 秒の VIVE Hub のログに、姿勢の途切れや切断が増えていないかを PM が見る（#7 の書式のメモのあと）。
- 開けて、VIVE Hub に影響がないと分かった場合に、VIVE Hub と並べて読む使い方を許すかは、DG-3 の変更になるので、PM が決める（PR の「未解決」）。

## 8. 記録の形（DG-4）

### 8.1 決まり

- **受け取った報告を、解読せずにそのまま**（16 進の文字列）、時刻つきで残す。解読の誤りが分かっても、記録を取り直さずに解読し直せる。
- 形は JSON Lines（UTF-8、1 行に 1 つの JSON）。1 行目が見出し、あとは報告と覚え書き、最後に締め。
- 読む側は、知らない `type` の行と、知らない欄を無視する。壊れた行（JSON として読めない、途中で切れた最後の行）は飛ばして数える。締めの行がなくても読める（Ctrl+C で止めたとき）。
- 形を互換でなく変えるときは、`version` を上げる。読む側は、知らない `version` を断る。

### 8.2 行の種類

値は例（`usage_page` などは、試し A で分かるまで決まっていない）。

```jsonl
{"type":"header","format":"openut.dongle-record","version":1,"tool":"openut-dongle 0.1.0","mode":"record","started_wall_ms":1764302512123,"device":{"vid":"0bb4","pid":"0350","interface":0,"usage_page":"ff00","usage":"0001"},"server_running_at_start":false,"anonymized":false}
{"type":"in","mono_us":1520,"wall_ms":1764302512125,"hex":"28a1003a…"}
{"type":"note","mono_us":60000000,"wall_ms":1764302572123,"text":"server_appeared"}
{"type":"footer","mono_us":120000000,"reports":29280,"malformed":0,"estimated_lost":12,"reason":"duration"}
```

| 欄 | 意味 |
|---|---|
| `mode` | `record` か `probe-sharing` |
| `device` | 機器の種類だけ。**シリアル番号と、機器の通り道（パス）は入れない**（パスに機器ごとの番号が入るため） |
| `mono_us` | 記録の始まりからの単調な時刻（マイクロ秒）。並べ替えと再生はこれで行う |
| `wall_ms` | 受け取ったときの UTC の Unix 時刻（ミリ秒）。VIVE Hub のログとの突き合わせに使う |
| `hex` | 受け取った報告（hidapi が返したまま） |
| `note` の `text` | `server_appeared`、`read_error:<OS の番号>`、`device_removed`、`duration` など。機械で読める短い語 |
| `estimated_lost` | `0x28` の通し番号の飛びから見積もった取りこぼし（U-11 が分かるまでは目安） |

### 8.3 置き場所と大きさ

- 既定の置き場所は `%LOCALAPPDATA%\OpenUT\dongle\`、名前は `YYYYMMDD-HHMMSS-<mode>.jsonl`（UTC）。`--out` で変えられる。利用者の名前をコードに書かない（W-9 と同じ考え）。
- 毎秒約 244 件（U-16）なら、1 行約 190 バイトで、毎分約 2.8 MB、10 分で約 28 MB。`record` の長さは既定 10 分、最大 60 分。
- **記録そのものには、トラッカーの宛先、シリアル番号、Wi-Fi の名前とパスワード（`WH`）が入りうる。** 記録はオーナーの PC の中だけに置き、レポジトリと Issue には、8.4 の書き出しを通したものだけを入れる。`record` の終わりに、そのことを一行で知らせる。

### 8.4 匿名化した書き出し（`export --anonymize`）

L-1（#8 の匿名化の道具）と同じ考えで、バイナリの報告の中の決まった場所を置き換える。

1. **トラッカーの識別子**（`0x28` と `0x18` の宛先の 4〜8 バイト目）を、トラッカーの名前ごとに決まった値に置き換える。4 バイト目は下位 4 ビット（枠の番号の推定）だけ残して上位を `0xA`、5〜7 バイト目を `0x00`、8 バイト目を名前の番号（A = 1、B = 2…）にする。さらに、元の 5 バイトの並びが**ほかの報告（`0x29` など）のどこかに現れたら、同じ値に置き換える**。
2. **個人の情報を含む文字列の知らせ**（6.3 の一覧）は、種類の文字を残し、値を**同じ長さ**の置き換えにする（長さの欄を壊さないため）。置き換えは、#8 の置き換えの表があればそれを使い（ファイルをまたいで同じ値は同じ置き換え）、なければ値ごとに決まった文字列（`X` で埋めて末尾に番号）にする。
3. **知らない文字列の知らせ**は、先頭 2 文字を残し、残りを `X` にする。件数を出す。
4. 見出しの `anonymized` を `true` にする。
5. **残っていないかの確認**: 1・2 で置き換えた元の値（5 バイトの並び、文字列）を集め、書き出した結果のすべての報告のバイトと文字の中に、ひとつも残っていないことを確かめる。残っていたら、ファイルを書かずに終わる（終了コード 5）。
6. 姿勢（位置と向き）は残す。体の動きの記録にあたるので、オーナー以外の利用者から集めるときは L-4 の決まり（送る前に示す）に従う。

## 9. 道具の使い方

### 9.1 命令（サブコマンド）

| 命令 | 何をするか | 動く OS | ドングルに触るか |
|---|---|---|---|
| `openut-dongle list` | VID `0x0BB4` の HID 機器を並べる（VID、PID、インターフェース、用途、製品名）。シリアル番号とパスは出さない | Windows | 一覧を取るだけ。hidapi は、読み書きの権限なしで開いて属性を読み、製品名などの文字列を USB の標準の要求で読む（S5）。機器の命令は送らない |
| `openut-dongle check` | サーバーが動いているか、ドングルがあるかを出す。読み取りを始められるかの答え | Windows | 開かない |
| `openut-dongle record [--minutes N] [--out PATH]` | 読んで、記録しながら、1 秒ごとに状態を出す | Windows | 開いて読む（送らない） |
| `openut-dongle probe-sharing [--seconds N]` | 前提の確認（7.3）。N は既定 30、最大 60 | Windows | 開いて読む（送らない） |
| `openut-dongle decode RECORD` | 記録を解読して、出来事を 1 行ずつ JSON で出す（M3 の再生テストの期待値づくりにも使う） | どれでも | 触らない |
| `openut-dongle summary RECORD [--utc-offset +9]` | トラッカーごとに、見えた時間、追跡の状態ごとの件数と切り替わりの時刻、地図の状態、知らない報告の件数、取りこぼしの見積もりを出す（VIVE Hub のログとの突き合わせ用） | どれでも | 触らない |
| `openut-dongle export --anonymize RECORD OUT` | 8.4 の書き出し | どれでも | 触らない |

終了コード: 0 正常、1 使い方の誤り、2 VIVE Hub のサーバーが動いている、3 ドングルがない・開けない・外れた、4 記録を書けない、5 匿名化の確認で元の値が残った。

### 9.2 何をいつ見せるか（コンソール）

- **始めに**: 見つけたドングルの種類、記録の置き場所、止め方（Ctrl+C か、時間が来たら止まる）。
- **1 秒ごとに**: 経過の秒数、ドングル全体の毎秒の受信の件数と取りこぼしの見積もり、トラッカーごとに 1 行（名前、つながり、追跡の状態の番号と意味、位置 x・y・z（m、小数 2 けた）、姿勢の毎秒の件数、地図の状態、ホストかどうか（分からなければ「不明」）、電池とボタンのバイト（16 進のまま））。
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
- 読み取りのスレッドは、`read_timeout`（100 ms）で回し、受け取ったらすぐ時刻を付けて、チャンネルで主のスレッドへ渡す。主のスレッドが、記録、解読、表示、見張りを行う。入力のためは hidapi が 64 件にする（`m2-dongle-protocol.md` の 7 章）。毎秒約 244 件なら、読み取りが 0.25 秒止まるとあふれる見込みなので、読み取りのスレッドでは書き込みも解読もしない。

## 11. オーナーに試してもらう手順の案（10 分以内。どれも何も送らない）

**この手順は案。** レビュー担当の承認と PM の確認のあと、M2-5（13 章）が main に入ってから、PM が `needs-human` の Issue にする（Issue #6 の決まり）。

準備（時間に入れない。PM が段取りする）: 道具の実行ファイルをオーナーの PC に置く。VIVE Hub で 3 台が使える状態にする。HMD は着けなくてよい。SteamVR は閉じておく。

| 試し | 時間 | 手順 | 見てほしいこと | 分かること |
|---|---|---|---|---|
| A 一覧 | 1 分 | 1. VIVE Hub を動かしたまま `openut-dongle list` を実行する。 2. 続けて `openut-dongle check` を実行する | 1 の出力（ドングルの行が出るか）。2 で「サーバーが動いているので始めない」と出るか | U-3、DG-3 の断り |
| B 前提の確認 | 2 分 | 1. VIVE Hub の画面で、3 台が追跡中であることを確かめる。 2. トラッカーを動かさずに `openut-dongle probe-sharing` を実行する（30 秒）。 3. 終わったら、VIVE Hub の画面で 3 台がまだ追跡中かを見る | 2 の出力（開けたか、件数）。3 の結果 | U-2、U-17。記録は、同じ時刻の VIVE Hub のログと突き合わせる |
| C サーバーを止めて読む | 5 分 | 1. 3 台を追跡中のまま、VIVE Hub を終了する（タスクトレイから）。 2. タスク マネージャーで `ViveTrackerServer.exe` が消えたかを見る。残っていれば「タスクの終了」をする（PC のプログラムを止めるだけで、機器への命令ではない）。立ち上がり直したら、そのことを書いて C をやめる。 3. すぐに `openut-dongle record --minutes 3` を実行する。 4. 1 台（足のどれか）を手に取り、30 cm ほど動かして戻す。 5. 1 分たったら、ホストでない足の 1 台の電源を切り、10 秒後に入れ直す。 6. 3 分で自動で止まるのを待つ | 2 の結果。3〜6 のあいだの画面の行（件数、追跡の状態、位置が動いたか）。5 で「外れた」「つながった」が出たか | U-1（(b) と、5 で (a) の一部）、U-4〜U-9、U-11〜U-16 |
| D 後片付け | 1 分 | 1. VIVE Hub を起動し、3 台が追跡を始めるかを見る。始まらなければ、いつもの手順で電源を入れ直す | 始まったか | 試しの影響が残らないこと |

報告: A と B の画面の出力を貼る（個人の情報は出ない作り）。B と C の記録は、`openut-dongle export --anonymize` を通したものを、オーナーの PC につながった Claude のチャットで PM に渡す（クラウドの担当はオーナーの PC を読めない。OPERATING.md の 7 章）。あわせて、試しの時間帯の VIVE Hub のログ（#7 の書式で匿名化したもの）も渡す。

危険の見込み: どの試しも、ドングルとトラッカーに何も送らない。B では、VIVE Hub が使っているドングルを、こちらも共有で開いて読む。こちらの読み取りが VIVE Hub の受け取りを減らす作りだった場合（U-2）、30 秒のあいだ VIVE Hub の追跡が乱れうる（装着していないときに行う。3 で確かめる）。C では、追跡中にサーバーを止めるので、トラッカーが待機や切断に入りうる（U-13）。D で元に戻す。

## 12. 仮の値と決め方

| 値 | 仮の値 | 決め方 |
|---|---|---|
| 途切れたとみなすまでの時間 | 2.0 秒 | 試し C の記録で、トラッカーごとの受信の間隔の分布を出し、ふつうの最大の間隔の 4 倍（ただし 1 秒以上）にする |
| 表示の間隔 | 1 秒 | 変えない |
| 姿勢の毎秒の件数の窓 | 直前 1 秒 | 変えない |
| サーバーの見張りの間隔 | 2 秒 | 変えない（プロセスの一覧を取る手間は小さい） |
| `record` の長さ | 既定 10 分、最大 60 分 | 試し C の記録の大きさを見て、PM が見直す |
| `probe-sharing` の長さ | 30 秒、最大 60 秒 | 変えない |
| 読み取りの待ち | 100 ms | 変えない |

## 13. 実装の項目（PR 1 本の大きさ）

**置き場所**: M2 のコードは、M1 の実装計画（#5 の `implementation-plan.md`）が決めるワークスペースに、クレート `openut-dongle`（ライブラリと、同じ名前の実行ファイル）として置く。ワークスペースの決まりがほかの形なら、それに従う。この節の項目は、#5 が main に入ったあと、PM の追補の依頼で `implementation-plan.md` に移す。

**順序の決まり**: M2-1 は、#5 の実装計画が main に入ってから（ワークスペースの分け方に合わせるため）。M2-1 → M2-2 → M2-3 の順。M2-4 と M2-5 は、M2-3 のあと、どちらが先でもよい。M2-4 は、#8（匿名化の道具）が main に入っていれば、その置き換えの表を使う。

| 項目 | 範囲のファイル（案） | 受け入れ条件（確かめるテスト） |
|---|---|---|
| **M2-1 記録の形** | `crates/openut-dongle/Cargo.toml`、`src/lib.rs`、`src/record.rs`、`tests/record.rs`、`tests/fixtures/dongle/`（手で作った見本の記録） | (1) 書いて読むと同じものに戻る。(2) 知らない `type` の行と知らない欄を無視する。(3) 壊れた行と、途中で切れた最後の行を飛ばして数える。(4) 締めの行がなくても読める。(5) 知らない `version` を断る。(6) 見出しに `device` の決まった欄しか書けない（シリアル番号やパスを入れる口がない） |
| **M2-2 報告の解読** | `src/decode/`（`mod.rs`、`pose.rs`、`text.rs`、`rf_status.rs`）、`src/half.rs`、`tests/decode.rs` | (1) この設計の表（`m2-dongle-protocol.md` の 4 章）から手で組んだバイト列が、期待した出来事になる（姿勢の向きの並べ替え、文字列の種類の表のすべての行、`0x18`、`0x1E` の枠の印）。(2) 16 ビットの浮動小数点の変換（0、1、-2、非正規化数、無限大、NaN）。(3) 先頭に 0 が残った報告を 1 バイトずらして読む。(4) 長さが足りない報告は `Malformed`。(5) 決まった種から作った 1 万件のでたらめなバイト列で、パニックしない（依存を増やさず、簡単な乱数を自前で作る）。(6) 個人の情報を含む種類の値が `Debug` の出力に出ない |
| **M2-3 状態表と要約** | `src/tracker.rs`、`src/summary.rs`、`src/view.rs`、`src/main.rs`（`decode`・`summary`） | (1) 見本の記録（3 台、1 台が外れてつながり直す、追跡の状態が 2→3→2 と変わる）を流すと、期待した `TrackerView` の移り変わりになる。(2) 名前は初めて見た順に A、B、C。(3) 途切れの判定が `mono_us` だけで決まる（時計を読まない）。(4) `summary` の出力が期待した文字列になる。(5) 表示の文字列に、宛先、シリアル番号、Wi-Fi の値が出ない |
| **M2-4 匿名化した書き出し** | `src/anonymize.rs`、`src/main.rs`（`export`） | (1) にせのシリアル番号（`NADS`）、にせの `WH` の値、にせの識別子（`0x28` と `0x29` の両方に現れる）を入れた見本を書き出すと、元の値がひとつも残らない。(2) 同じ値は同じ置き換えになる。(3) 長さの欄が壊れない（書き出したものを解読すると、同じ種類の出来事になる）。(4) 残りの確認で見つかったときに、ファイルを書かずに終了コード 5 で終わる（確認を通り抜ける見本をわざと作って確かめる）。(5) 見出しが `anonymized: true` |
| **M2-5 Windows の入出力と道具** | `src/guard.rs`、`src/source_windows.rs`、`src/process_windows.rs`、`src/main.rs`（`list`・`check`・`record`・`probe-sharing`）、`Cargo.toml`（Windows だけの依存） | (1) サーバーの判定（名前の大小文字の違い、名前の足し方、空の一覧）。(2) **送らないことの検査**（4 章の 3。`hidapi` の名前が `src/source_windows.rs` だけにあり、禁じた関数の名前がどこにもない）。(3) `record` の時間切れ、サーバーが途中で現れたとき、読み取りの誤りのときに、記録に `note` と締めが書かれる（読み取りの部分を差し替えられる形にして、Linux でもテストする）。(4) Windows の CI で組み立てと clippy が通る。実機での確かめは、このあと 11 章の手順で行う（PR の受け入れ条件ではない） |
| M2-6（条件つき） | 問い合わせ Q1〜Q3 のうち了承されたもの | 試し C の結果と、PM・オーナーの了承があってから、設計の追補で中身を決める。いまは作らない |

## 14. M3 への引き継ぎ

- そのまま使うもの: 記録の形（M3 の再生テストの入力）、解読、トラッカーの状態表、匿名化した書き出し、サーバーの見張り（M3 でも、サーバーが動いていたら起動を断る）。
- M3 で足すもの: 送る部分（`commands.md` の 6 章の形）、M1 の状態と検出の規則への差し替え（R-9。`m2-dongle-protocol.md` の 8 章の表を、試し B・C の記録で確かめてから）、SteamVR への受け渡し。
- 試し C で、PC が送らないと姿勢が届かないと分かった場合、M3 の最初の仕事は、追跡を始める命令（`commands.md` の 4.1）の確かさを上げることになる。
