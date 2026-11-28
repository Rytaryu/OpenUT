# VIVE Hub のログの書式（PC 側）

オーナーの PC にある VIVE Hub のログを読んで、書式をまとめた（Issue #7）。見張り役（M1）の読み取りと、匿名化の道具（PRD の L-1）の仕様のもとにする。行の見本は `hub-log-samples.md`。

**確かめた範囲。** オーナーの PC 1 台、トラッカー 3 台、2026-11-23〜11-28 の 6 日分。版は、ログに出ていた値で、`ViveTrackerServer.exe` 1.1.4.28、`vive_tracker_driver.dll` 1.0.7.98、トラッカーのファームウェア 1.0.999.178、ドングル 0909.0000.0000.0917。ほかの版・ほかの PC で同じとは限らない。数は、PM がスクリプトで数えたもの（元のログはレポジトリに入れていないので、読み手は数え直せない）。「推定」と書いたところは、値からの読み取りで、確かめていない。

## 1. 場所とファイル

- フォルダー: `%ProgramData%\HTC\ViveSoftware\ViveUltimateTracker\Log`（利用者のフォルダー名を含まない）。
- 1 日 1 ファイルで、名前の末尾が日付（PC の現地の日付）。見張り役と匿名化に関わるのは、次の 3 種類。

| ファイル | 書いているもの | 1 日の行数 | 1 日の大きさ |
|---|---|---|---|
| `ViveTrackerServer_YYYYMMDD.txt` | トラッカー用のサーバー（`ViveTrackerServer.exe`）。接続、ホストの割り当て、Wi-Fi の受け渡し、地図、姿勢の状態 | 2,002〜15,828 | 0.7〜5.5 MB |
| `ViveTrackerDriver_YYYYMMDD.txt` | ドングルとの通信（HID）の層。スロットへの接続、電源の状態、トラッカーの電源オフの知らせ | 1,957〜18,775（うち 9 割以上が、下の 3.3 の「待ちの時間切れ」） | 0.5〜4.6 MB |
| `ViveTrackerDongle_YYYYMMDD.txt` | ドングルの無線の統計（取りこぼしの割合、電波の強さ）と、無線の接続・切断 | 1,355〜5,600 | 0.6〜2.7 MB |

- 同じフォルダーには、ほかに `TrackerStatisticsReport.csv`、`TrackerLatencyReport.csv`、`ManifestAutoRegister_YYYYMMDD.txt`、`SteamVRNotificationHelper.txt`、取り出しツールのログなどがある。中身の書式は調べていない（4 章の語句の検索だけ行った）。
- 残る期間: 11/28 の時点で、3 種類とも 11/23〜11/28 の 6 ファイルがあった。11/27 に読めた 11/22 のファイルは、なくなっていた。ヘッダーの `MaxRollingCount=5` と合う（今日の分と、過去 5 日分。推定）。

## 2. 入れ物の書式（3 種類に共通）

### 2.1 文字コードと改行

- **UTF-16LE、BOM つき**（先頭の 2 バイトが `FF FE`）。18 ファイルすべて。6 日分に、ASCII の外の文字は 0 個。
- **1 件の記録の終わりは CRLF**。ファイルの最後の記録も CRLF で終わる。
- **本文の中に、LF だけの改行が入ることがある**。記録を LF で切ると、1 件が 2 行に割れる。CRLF で切ること。
  - サーバー: 6 日で 102 件（`Save map creation time: <曜日 月 日 時刻 年>\n.` の形）。
  - ドライバー: 47,648 件（`Wait for HID data got timeout.\n`。本文の末尾が LF で、そのあとに記録の終わりの CRLF）。
  - ドングル: 0 件。

### 2.2 記録の先頭の形

```
[2026-11-27 22:53:33.658 +9 0xF88 WARN OETServer::ProcessDeviceMsg] [3976] Wifi info is not yet available.
 └──────── 時刻 ────────┘ │  │      │    └────── 関数名 ──────┘      │      └ 本文
                          │  │      └ レベル                         └ スレッド番号（10 進）
                          │  └ スレッド番号（16 進）
                          └ UTC からのずれ
```

- 時刻は、PC の現地の時刻。ミリ秒は 3 桁。
- `+9` は、111,131 件すべてで同じ。日本の PC なので、UTC からのずれ（時）と読める（推定。ほかの時間帯の PC では確かめていない。30 分単位のずれがどう書かれるかも分からない）。
- `0x....` は、スレッド番号の 16 進。大文字で、桁をそろえない（`0x3410`、`0xF88`）。**後ろの `[....]` の 10 進と、全件で同じ値だった。**
- レベルは `INFO`、`WARN`、`ERROR` の 3 つが出ていた（ヘッダーは `InitialLogLevel=2`）。
- 関数名は `クラス::関数` の形が多い。`::` のないもの（`main`、`GetIPThread`、`dumpPUMRFDropRate`）、`::` が 2 つのもの（`bisender::BiSender::Init`）、`~` が入るもの（`CSocketAccept::~CSocketAccept`）がある。空白は入らない。**関数名が空のものがある**（`... INFO ] [13328] ==================== Log Start ====================`。レベルのあとに空白 1 つで `]`）。
- 本文が空の記録がある（`[スレッド番号] ` のあとの空白 1 つで終わる。サーバーの `BiCollector::GetPcSerialNumber` の `ERROR` が 2 件、ドライバーの `TrackerDongleHID::GetSerialNumber` の `ERROR` が 8 件）。
- 1 つのファイルの中で、時刻が前に戻ることはなかった（サーバーの 6 ファイル）。同じミリ秒の記録が続くことは多い。
- 同じ本文が 2 回続けて出ることが多い（例: `Remove tracker (2), ...` が 3 ミリ秒おいて 2 回）。同じ出来事を 2 回数えない作りが要る。

### 2.3 `#` で始まる行（ヘッダー）

記録の形をしていない行は、`# ` で始まるものだけだった。

- **プロセスが起動するたびに**、`# Log begin at 2026-11-27 19:07:26.866` と、設定の 13 行が書かれる（見本の 1）。同じ日に起動し直すと、ファイルの途中に足される（11/23 は 5 回）。**途中で切り詰められたファイルはなかった**（`Truncate=0`）。
- **日付が変わって作られたファイルは**、先頭が `# Log rotate by date at 2026-11-27 00:01:03.40`。設定の 13 行は付かない。
- ヘッダーの時刻のミリ秒は、桁をそろえない（`.40` は 40 ミリ秒、`.60` は 60 ミリ秒）。
- 設定の値は 3 種類とも同じ: `WriteTimeInterval=5000`、`OpenType=1`、`FileType=0`、`EncodeType=0`、`Truncate=0`、`MaxFileSize=15728640`、`MaxRollingCount=5`、`WriteThrough=1`、`HardFlush=0`、`FilenameWithProcessID=0`、`InitialLogLevel=2`、`ThreadPriority=0`、`AllPackagesAccess=0`。

### 2.4 まとめて書かれる間隔

- ヘッダーの `WriteTimeInterval=5000` から、**5,000 ミリ秒ごと**と読める（推定。ファイルが伸びる様子を横で見て測ってはいない）。設計の L（仮の 10 秒）は、これより長い。

### 2.5 日付の切り替わり

日付が変わったあとの**最初の書き込みのとき**に切り替わる（0 時ちょうどではない）。サーバーの 5 回の切り替わりは、どれも次の形だった。

1. 古い日のファイルの最後に、**次の日の日付の記録が 1 件**入る（例: `ViveTrackerServer_20261123.txt` の最後が `[2026-11-24 00:02:11.121 ...`）。
2. 新しい日のファイルが作られ、先頭が `# Log rotate by date at 2026-11-24 00:02:11.122`。次の記録から、新しいファイルに入る。

- 切り替わった時刻は、サーバーで 00:00:06〜00:02:11。記録が少ないドライバーでは、00:12、00:51、01:41 のこともあった（その時刻まで、古い日のファイルに書き足されていた）。
- 0 時をまたいだあとにプロセスが起動した日（サーバーの 11/28）は、新しいファイルが `# Log begin at` で始まる。
- `MaxFileSize=15728640`（15 MB）を超えたときの動きは、分からない（最も大きいファイルで 5.5 MB）。同じフォルダーの別のツールのログには、名前の末尾が `_1` のファイルがあった。

### 2.6 書いている最中に読めるか（未確認）

VIVE Hub が書いている最中のファイルを、ほかのプログラムが開いて読めるかは、確かめていない（`OpenType=1`、`WriteThrough=1` の意味が分からない）。設計の 14 章の手順で、オーナーに試してもらう。

## 3. 行の種類

### 3.1 サーバー（`ViveTrackerServer_*.txt`）

設計（`m1-watcher.md` の 4.4）の表の行は、**すべてサーバーのファイルにある**（ドライバーとドングルのファイルには、ない）。綴りは下のとおり。`N` はトラッカーの番号（3.5）。1 日の数は、6 日の最小〜最大。

| 設計の種類 | 関数名 | 本文（`N`・`V` は数） | 1 日の数 |
|---|---|---|---|
| TrackerAdded | `OETServer::AddTrackerCB` | `Add new tracker: (N), connection type: V` | 3〜27 |
| TrackerRemoved | `OETServer::RemoveTrackerCB` | `Remove tracker (N), connection type: V, power state:V `（`power state:` のあとに空白がなく、末尾に空白が 1 つある） | 2〜45 |
| HostMode | `Tracker::UpdateSwitchingMode` | `Tracker (N) - switch to HOST mode.` | 1〜9 |
| （新） | 同上 | `Tracker (N) - switch to CLIENT mode.` | 1〜8 |
| HostWifiQuery | `OETServer::TrackerSwitchAndResponse` | `Query wifi from tracker N`（括弧なし） | 1〜9 |
| HostWifiGot | `OETServer::ProcessDeviceMsg` | `Got host wifi info.`（**番号なし**） | 2〜15 |
| WifiAssign | 同上 | `Assign wifi info to client tracker (index: N).` | 2〜25 |
| WifiNotYet | 同上（`WARN`） | `Wifi info is not yet available.`（**番号なし**） | 0（4 日）、671、2,071 |
| ClientLink | 同上 | `Client tracker (index: N) is connected to host: V`（V は 0 か 1） | 3〜30 |
| MapState・MapStatus | `OETServer::DeviceStatusToVHServer` | **単独の行ではない。** 状態の行（下）の中の `map status: V`、`map state: V` | 104〜802 |
| Pose | `Tracker::CopyFromDriverThreadProc` | `(SteamVR: S)(N)[K]Pose status is changed: Pose data - status: V, pos: (x, y, z), rotation: (x, y, z, w w), vel: (x, y, z), angular vel: (x, y, z) `（末尾に空白） | 31〜338 |
| OetLost | `OETServer::OnTrackerErrorNotification`（`ERROR`） | `OET LOST TRACKING! index: N` | 3〜25 |
| OetReloc | 同上（`INFO`） | `OET reloc back. index: N` | 5〜23 |
| OetOverspeed | `OETServer::OnTrackerStatusIndicatorUpdate`（`ERROR`） | `[N] OET OVERSPEED!` | 4〜26 |

設計の表と違っていた点:

- **`map state:` と `map status:` は、状態の行の一部**。状態の行は次の形で、トラッカー 1 台について 1 行（番号は `device index`）。

  ```
  display index: 0, device index: 2, connected: 1, tracking status: 2, charging: 0, percentage: 74, map ID: 0, map status: 6, has map: 1, map state: 2, is host: 0, bIsConnectedOnUSB: 0, has new fw: 0, is unpaired: 0, last update time: 0, error code: -1, indicator event: 0, indicator value: 0
  ```

  - 値は負のこともある（`map ID: -1`、`map status: -1`、`error code: -1`）。
  - 出ていた値: `connected` 0・1、`tracking status` 0・2・3・4・5・6・7、`map status` -1・0・1・2・3・6・10、`map state` 0・1・2、`is host` 0・1。意味は、この調べでは確かめていない（`map state: 2` と `map status: 6` の意味は、初版の調べのまま）。**`tracking status` には、姿勢の行の値（2・3・4）にない 5・6・7 が出る**ので、同じ番号づけではないかもしれない。
  - 出る間合い: 出来事のとき（接続、状態の変化）にまとまって出る。変化がないときも、接続中のトラッカーごとに、数分おきに出ていた（同じトラッカーの行の間隔は、9 割が 462 秒以内、99% が 687 秒以内、最大 2,503 秒）。
  - 同じ時刻に、サービス全体の行（`[ServiceStatusToVHServer] dongle connected: 1, tracker connection type: 0, scan status: 1, device counts: 3, cali status: 1, error code: 0, indicator event: 0, indicator value: 0, camera fps: 50`）と、`Host running state:V to V`、`Scan status: V, Cali status: V` が続くことが多い。
- **姿勢の行は、状態が変わったときだけ出る**（本文に `Pose status is changed:` とある）。定期的には出ない。
  - `S` は SteamVR 側の番号（つながる前は `-1`）、`N` はトラッカーの番号、`K` は 0〜255 の数（意味は分からない）。
  - `status` の値: 2 が 267 件、4 が 234 件、3 が 76 件、0 が 29 件。ほかに、接続の直後に 1〜4 件ずつ、意味のなさそうな値（47、58、71、110、175 など）が出る。**知らない値で止まらないこと。**
  - 位置や回転に `-nan` が入ることがある。接続の直後には、桁の多い値（`-42535295865117307932921825928971026432.000000`）も出る。
- **`Wifi info is not yet available.` は、出るときは続けて出る**。間隔の中央値は 0.37 秒（9 割が 0.56 秒以内）。11/26 01:43〜01:52 に 2,071 件、11/27 22:53〜23:01 に 671 件。
- 大文字と小文字は、上の表のとおりで、6 日分では揺れがなかった。

状態や検出に関わる、ほかの行:

| 分かること | 関数名 | 本文 | 1 日の数 |
|---|---|---|---|
| サーバーの開始（U-10） | （空）、`main` | `==================== Log Start ====================`、`Tracker Server Starts.` | 1〜5 |
| サーバーの終了 | `OETServer::Release`、（空） | `Release begin`、`Release end`、`-------------------- Log End --------------------` | 0〜4 |
| 番号とシリアル番号の対応（U-6） | `OETServer::GetDeviceInfoAndResponse` | `Response - cmd type: V, cmd key: (...), device index: N, result: V, sn: (シリアル番号), model name: (VIVE Ultimate Tracker V), fw version: (1.0.999.178), role: (V), device type: V, force update: V, type: V` | 6 日で 310 |
| ドングル（U-12） | `OETServer::OnDongleAddedCB`、`OnDongleRemovedCB`、`SetDongleToPowerSavingMode`、`WakeupDongle` | `Dongle is added. version: (...)`、`Dongle is removed.`、`dongle count: V, tracker count: V`、`Set dongle into standby.`、`Wake up dongle.`、`Wake up dongle: (N).` | 下の 3.5 |
| ペアリングの操作 | `OETServer::PairTracker`、`CancelPairing` | `Pair tracker, isPair: V, device index:N`（N は -2、-3 のこともある）、`Unpair All`、`Cancel pairing on dongle.` | 6 日で 279 |
| 電源の状態とホスト | `Tracker::UpdateSwitchingMode`、`OETServer::CreateTracker` | `power state: V, is host: V`、`Tracker (N) Switching mode: ON`／`OFF`、`Create tracker (N) - power state: V, data format: V, host running state: V`、`Start host tracker. ret: V` | — |
| SteamVR | `OETServer::OnSteamVrRunningChanged`、`OnPipeClosed` | `SteamVR Running is changed to V`、`Pipe closed. client: SteamVR` | 6 日で 35、7 |
| スリープ | `OETServer::OnTrackerErrorNotification` | `Tracker N entered sleeping (lost tracking)` | 6 日で 2 |
| エラーの番号 | `ErrorCodeManager::SetSystemErrorCode`、`ResetSystemErrorCode` | `error code: 3100 received`、`Error: 3100 fixed.` | 6 日で 6 ずつ |

数が多くて、検出には使わない行（読み飛ばしが速いこと）:

| 本文 | 1 日の数 |
|---|---|
| `(SteamVR: S)(N)(tracking status: V) Position (-nan, -nan, -nan) includes NAN.`、`... Rotation (-nan, -nan, -nan, -nan) includes NAN.`（`WARN`。`-nan(ind)` の形もある） | 0〜8,646 |
| `OnPipeClientError::OnPipeError (16 桁の 16 進) : WriteFile Fail : V`（`ERROR`） | 1〜1,211 |
| `Tracker is not found`（`WARN`。`OETServer::OnTrackerNotificationMsg`） | 40〜430 |

### 3.2 PC からトラッカーへの命令にあたる行（U-16）

サーバーのファイルで、PC（VIVE Hub）がトラッカーやドングルに何かをさせたと読める行。R4（操作と関係のない切断）の除外の材料。

- `Tracker (N) - switch to HOST mode.`、`Tracker (N) - switch to CLIENT mode.`
- `Query wifi from tracker N`、`Assign wifi info to client tracker (index: N).`
- `Start tracker - power state :V, data format: V`、`Start tracker. state: V, type: V, ret: V`、`Start host tracker. ret: V`、`Stop tracker.`
- `Pair tracker, isPair: V, device index:N`、`Unpair All`、`Cancel pairing on dongle.`
- `Wake up dongle.`、`Wake up dongle: (N).`、`Set dongle into standby.`
- `Camera frame rate: V`（`OETServer::SetCameraFrameRateToAllTrackers`）
- 失敗の行: `PowerOff tracker (N) failed, err: V`、`FAILED TO POWER OFF. index: V`、`Fail to stop tracker, err: V`

### 3.3 ドライバー（`ViveTrackerDriver_*.txt`）

- 9 割以上が `TrackerDongleHID::DataDispatchThreadProc` の `Wait for HID data got timeout.\n`（6 日で 47,641 件）。
- トラッカーの番号は、`slot num: N`、`slot N`、`(N)`、`[N]` の形で出る（サーバーの `device index` と同じ 0〜4）。
- 見張り役に関わりそうな行:
  - **`(N) Tracker is going to power-off.`**（`TrackerDongleHID::ParseIndication`）: トラッカーが、電源を切る前に知らせてくる行（3.5）。
  - `Tracker is paired to slot N, connected: V`、`Add tracker. slot num: N, type: V.`、`Remove tracker. slot num: N, type: V`
  - `(N) set power state: V`、`(N) Set is host: V`、`(N) Set has_map to: V`、`Query wifi info from tracker N`、`Assign wifi info.`
  - `[N] Tracker connected.`、`[N] CONNECTION TIMEOUT! unpair tracker`、`[N] Tracker failed to connect. retry: V.`
  - `Dongle (16 桁の 16 進) is added`／`is removed`、`Dongle version: (...)`、`DBT_DEVICEARRIVAL`、`DBT_DEVICEREMOVECOMPLETE`

### 3.4 ドングル（`ViveTrackerDongle_*.txt`）

- 関数名は 3 つだけ: `dumpPUMRFDropRate`（6 日で 17,454 件。中央値で 2.9 秒おき）、`dumpPUMRFQueryStatus`、`dumpPUMRFConnect`。
- `dumpPUMRFDropRate`: `  - D[N][RSSI: -83 ~ -52][CH: 42]DropRate: 1.50 % (15/1000), MAX RF-LOST:  2 InvalidFmt (   0,   0,   0,   0) Cont.InvalidFmt+Drop:   2, Total DropRate: 1.50 % (15/1000) Recv(0,985)`（本文の先頭に空白 2 つ。数の前の空白の数は、桁でそろえてあって一定でない）。
- `dumpPUMRFConnect`: `  - Change Status:    Connected, bt mac address: (#N)(HH HH HH HH HH HH)`、`  - Change Status: Disconnected, bt mac address: (#N)(HH HH HH HH HH HH)`。**無線のアドレスが入る**（4 章）。
- M2 の試し（VIVE Hub 側の取りこぼしが増えないかを見る。`m2-dongle-reader.md` の B-3）では、`DropRate` の行が使える。

### 3.5 トラッカーの番号（U-6）、切断の理由（U-11）、ドングル（U-12）、行の空き（U-7）

**番号 N。**

- 0〜4 の 5 通り（ドングルのスロット。ドライバーは `slot` と呼ぶ）。サーバーの `device index`、`index`、`(N)`、`Tracker (N)`、`tracker N` は、同じ番号。
- **番号は、トラッカーに固定されていない。** 同じシリアル番号が、同じ日のうちに別の番号で出ている（例: 11/23 は、1 台が 0・1・2・3・4 のすべてで出た。ペアリングをし直した日）。番号をトラッカーの名前のキーにしてはいけない。
- 番号とシリアル番号の対応は、`GetDeviceInfoAndResponse` の行（3.1）で分かる。接続のたびに何度も出る。
- `display index`（0〜2）と `(SteamVR: S)` は、別の番号。`model name: (VIVE Ultimate Tracker V)` の V（1〜3）が台ごとに固定かは、確かめていない。

**切断の理由。**

- サーバーの `Remove tracker (N), connection type: V, power state:V ` は、6 日で 128 件。`connection type` は 1 が 109 件、2 が 19 件（2 は USB でつないだとき。推定）。`power state` は 11、21、2、100 が出ていた。
- **利用者が電源を切ったとき**: ドライバーのファイルに `(N) Tracker is going to power-off.` が出て、15 秒以内にサーバーの `Remove tracker (N)` が続く。128 件のうち 86 件が、この形（知らせは 44 件。サーバーの行は同じ出来事で 2 回出る）。
- 残りの 42 件は、知らせがない。USB の 19 件、ペアリングの操作（`Unpair All` など）のあとのもの、ほんとうに切れたものが混じっている。**サーバーのファイルだけでは、電源オフと、それ以外の切断を分けられない。** 分けるには、ドライバーのファイルも読む（設計の追補で決める）。

**ドングル。**

- つながったとき: `Dongle is added. version: (0909.0000.0000.0917)`（サーバーの起動のときと、挿し直したとき。6 日で 12 件）。
- 抜けたとき: `Dongle is removed.` と `dongle count: 0, tracker count: 0`（3 件。3 件とも 11/28 の 01:39〜02:56 で、オーナーがトラッカー内のログの取り出しをしていた時間帯らしい。推定）。このうち、ドングル経由でトラッカーが 2 台つながっていた 1 回（01:56）では、**その 2 台の `Remove tracker` は出なかった**。ドングルが抜けたら、ドングル経由の全台が切れたものとして扱う（1 回の観察による）。
- サーバーが終わるとき: `Set dongle into standby.`

**行の空き。**

- トラッカーが 1 台以上つながっているあいだの、サーバーの記録の間隔は、99% が 8.2 秒以内、99.9% が 270 秒以内。長いほうから 8 つは、20〜36 分が 3 回、8〜10 分が 5 回（長いものは、ドングルが抜けたあとなど、実際にはつながっていなかった時間を含むかもしれない）。
- プロセスが起動したまま、記録が何時間も出ないことがある（最長 10.6 時間。PC のスリープを含むかもしれない）。
- ドングルのファイルは、ドングルが動いているあいだ、2.9 秒おき（中央値）に `DropRate` の行が出る。「VIVE Hub が生きているか」を見るなら、こちらのほうが細かい。

**サーバーの開始と終了。**

- 開始の 2 行（3.1）は、6 日で 11 回。終了の行は 7 回。**4 回は、終了の行がないまま、次の開始になっている**（PC の終了や、強制の終了。推定）。終了の行がなくても、次の `# Log begin at` と開始の行で、状態を捨てること。
- 開始の直後には、`Dongle is added.`、設定の読み込み、前に登録したトラッカーの状態の行（`connected: 0`）が続く。

## 4. 個人の情報が入る場所

6 日分の 3 種類のファイルと、同じフォルダーのほかのファイルを、形と語句で検索した結果。匿名化の道具（L-1）は、下の「置き換える」を必ず置き換える。

| 情報 | 入る場所 | 形 | 置き換え |
|---|---|---|---|
| トラッカーのシリアル番号 | **サーバー**の 2 種類の行。(1) `OETServer::GetDeviceInfoAndResponse` の `sn: (...)`。6 日で 310 件。(2) `OETServer::DeviceInfoCB`（`ERROR`）の `TRACKER NOT FOUND. tracker: ...`。122 件 | 13 文字。数字 2、英大文字 2、数字 9 の順（3 台とも） | 置き換える。同じ値は同じ別名に（台の区別が、解析に要る） |
| 無線のアドレス | **ドングル**の `dumpPUMRFConnect` の `bt mac address: (#N)(HH HH HH HH HH HH)`。6 日で 139 件 | 16 進 2 桁（大文字）を、空白で区切って 6 つ。`:` や `-` で区切った形は、0 件 | 置き換える。3 台で 8 通りの値が出ていた（番号ごとに違う値になるらしい。理由は確かめていない） |
| トラッカー同士の Wi-Fi の名前とパスワード | **0 件**。`Assign wifi info ...`、`Got host wifi info.` の行は、値を書かない | — | この版・この設定（`Log level: 2`）では出ない。ログのレベルを上げたときに出るかは、分からない。道具は、`ssid`、`password`、`psk` の語のあとの値を、念のため置き換える |
| PC の名前 | **0 件**（オーナーの PC の名前で、大文字と小文字を区別せずに検索） | — | 念のため、道具は実行した PC の名前を検索して置き換える |
| 利用者のフォルダー名 | **0 件**（`\Users\` が 0 件）。出ていたパスは、`C:\Program Files\VIVE Hub\...` と `C:\Program Files (x86)\Steam\config\...` だけ | — | Steam を利用者のフォルダーの下に入れている PC では、`chaperone file path: (...)` の行に出るはず（推定）。道具は、`\Users\<名前>\` の `<名前>` を置き換える |

個人を指すものではないが、環境や使い方が分かる値（道具で消すか、送る人に見せて選ばせるかを、設計で決める）:

- 国（`BiCollector::GetCountryCode` の `geo name: <2 文字>`）、Windows の版（`Windows version: ...`）、PC の種類（`pc type: ...`）、ヘッドセットの機種（`Got hmd type: ...`）、動かしていたアプリの名前（`app: <名前> is running.`／`is closed.`）。
- 姿勢の値（姿勢の行の位置・回転・速度）と、空間の較正の値（`Transform - rotation:(...), pos:(...), scale: ...`、`Trans in chaperone: (...)`）。体や部屋の位置にあたる。
- 電池の残量（`percentage: V`、`battery percent: V`）と、時刻（使っていた時間帯）。
- プロセスの中のアドレスらしい 16 桁の 16 進（`Client 000001... is connected`、`OnPipeError (000002...)`、ドライバーの `Dongle (000000...)`。起動のたびに変わる）、`cmd key: (2〜4 文字の英数字)`、`handle: <16 進>`、スレッド番号、待ち受けのポート番号（`Listen on port V, socket V`）。個人の情報ではないが、見本では置き換えた。
- `BiCollector::GetPcSerialNumber` という関数の行がある。6 日分の本文は `pc type: Desktop` か空で、PC のシリアル番号は出ていない。ほかの PC では出るかもしれないので、道具は、この関数の行の本文を丸ごと消す。

同じフォルダーのほかのファイル（`TrackerStatisticsReport.csv`、`TrackerLatencyReport.csv`、`ManifestAutoRegister_*.txt`、`SteamVRNotificationHelper.txt`、取り出しツールのログ）は、シリアル番号の形、`:`・`-` 区切りの無線のアドレスの形、PC の名前、`\Users\` のどれも 0 件だった。書式は調べていないので、ログの収集（L-3）に入れるなら、別に調べる。

## 5. 設計の未確認の点（`m1-watcher.md` の 4.8）との対応

| 番号 | 分かったこと | 章 |
|---|---|---|
| U-1 | UTF-16LE、BOM つき | 2.1 |
| U-2 | 記録の終わりは CRLF。本文の中に LF がある | 2.1 |
| U-3 | 先頭の形。`0x....` はスレッド番号の 16 進で、後ろの 10 進と同じ値。レベルは 3 つ。関数名に空白はなく、空のことがある。`+9` は推定のまま | 2.2 |
| U-4 | ある（本文の中の LF）。`#` の行のほかに、記録の形をしていない行はなかった | 2.1、2.3 |
| U-5 | 綴りと、番号の有無。`map state`・`map status` は状態の行の一部。`Wifi info is not yet available.` と `Got host wifi info.` には番号がない | 3.1 |
| U-6 | 0〜4 のスロットの番号。つなぎ直しで変わる。シリアル番号との対応が出る行がある | 3.5 |
| U-7 | 姿勢の行は、変わったときだけ。行の空きは 99.9% が 270 秒以内で、10 分前後の空きもある | 3.1、3.5 |
| U-8 | ヘッダーの設定は 5,000 ミリ秒（実測はしていない） | 2.4 |
| U-9 | 日付が変わったあとの最初の書き込みで切り替わる。古いファイルに、次の日の記録が 1 件入る。起動のたびの切り詰めは、ない | 2.3、2.5 |
| U-10 | 開始と終了の行。終了の行がないまま次の開始になることがある | 3.5 |
| U-11 | サーバーのファイルだけでは分からない。ドライバーのファイルの `Tracker is going to power-off.` で分かる | 3.5 |
| U-12 | ドングルが付いた・抜けた・待機にした、の行。抜けたときは、トラッカーの `Remove tracker` が出ない | 3.5 |
| U-13 | **未確認のまま** | 2.6 |
| U-14 | `%ProgramData%\HTC\ViveSoftware\ViveUltimateTracker\Log` | 1 |
| U-15 | 行数と大きさ | 1 |
| U-16 | 命令にあたる行の一覧 | 3.2 |

設計の追補で決めてほしいこと（PM から設計担当へ）:

1. 地図の状態を、状態の行（`DeviceStatusToVHServer`）から取る形に直す。同じ行の `tracking status`、`is host`、`connected` を使うか。
2. 番号のない `Wifi info is not yet available.` と `Got host wifi info.` の扱い（R1 は、直前の `switch to HOST mode` と `Add new tracker` の並びから、どの台の話かを決められるか）。
3. 切断の理由のために、ドライバーのファイルも読むか（読むなら、2 つのファイルの時刻の突き合わせ）。
4. 番号ではなく、シリアル番号（`GetDeviceInfoAndResponse`）をトラッカーのキーにする。
5. 同じ出来事の行が 2 回出ることへの対処と、`includes NAN.` などの多い行の読み飛ばし。
6. 0 時の切り替わりで、古いファイルの最後に次の日の記録が 1 件入ること（集計の日付）。
