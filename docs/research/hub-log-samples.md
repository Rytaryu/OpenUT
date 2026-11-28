# VIVE Hub のログの見本（PC 側）

`hub-log-format.md` の見本。オーナーの PC の 2026-11-27 のログから抜いた行で、**個人の情報と、起動のたびに変わる値は、置き換えてある**。

- シリアル番号は `00XX000000001`〜`00XX000000003`（形は実物と同じ 13 文字）。無線のアドレスは `00 11 22 33 44 55`。
- 16 桁の 16 進（プロセスの中のアドレス）は `00000000000000NN`、`cmd key` は `(AAAA)`、`handle` は `00001000` から。
- 姿勢と較正の数は、0 でないものを `0.123456`（符号は元のまま）にした。`0.000000` と `-nan` は元のまま。
- 時刻、スレッド番号、状態の値、電池の残量、版は、元のまま。
- ここの文字は UTF-8 で、改行は LF。**実物は UTF-16LE（BOM つき）で、記録の終わりは CRLF**。本文の中の LF は `\n` と書いた。`（中略: n 行）` は、PM が省いた行の数。

再生テストに使う、切れ目のないログ（2026-11-22〜27）は、匿名化の道具（Issue #8）ができてから、その道具で作る（PRD の L-2）。

## 1. ファイルの先頭と、プロセスの起動

実物の先頭の 16 バイト（`# Log rot`）: `FF FE 23 00 20 00 4C 00 6F 00 67 00 20 00 72 00`

プロセスが起動したとき（ファイルの途中にも出る）:

```
# Log begin at 2026-11-28 00:00:53.613
# WriteTimeInterval=5000
# OpenType=1
# FileType=0
# EncodeType=0
# Truncate=0
# MaxFileSize=15728640
# MaxRollingCount=5
# WriteThrough=1
# HardFlush=0
# FilenameWithProcessID=0
# InitialLogLevel=2
# ThreadPriority=0
# AllPackagesAccess=0
[2026-11-28 00:00:53.613 +9 0x3410 INFO ] [13328] ==================== Log Start ====================
[2026-11-28 00:00:53.613 +9 0x3410 INFO main] [13328] Tracker Server Starts.
[2026-11-28 00:00:53.629 +9 0x3410 INFO OETServer::ReadConfigFile] [13328] Succeed to read config json.
[2026-11-28 00:00:53.629 +9 0x3410 INFO OETServer::ParseConfig] [13328] Log level: 2
[2026-11-28 00:00:53.629 +9 0x3410 INFO OETServer::ParseConfig] [13328] Camera frame rate: 50
[2026-11-28 00:00:53.633 +9 0x3410 INFO OETServer::LoadTracker] [13328] My folder path: (C:\Program Files\VIVE Hub\VIVE Ultimate Tracker\ViveUTServer)
[2026-11-28 00:00:53.984 +9 0x3410 INFO OETServer::OnDongleAddedCB] [13328] Dongle is added. version: (0909.0000.0000.0917)
[2026-11-28 00:00:53.984 +9 0x3410 INFO OETServer::WakeupDongle] [13328] Wake up dongle.
[2026-11-28 00:00:53.985 +9 0x3410 INFO OETServer::CheckHostIsStarted] [13328] Host running state:0 to 0
[2026-11-28 00:00:53.985 +9 0x3410 INFO OETServer::GetSetupStatus] [13328] Scan status: -1, Cali status: -1
[2026-11-28 00:00:53.990 +9 0x3410 INFO OETServer::DeviceStatusToVHServer] [13328] display index: 0, device index: 2, connected: 0, tracking status: 0, charging: 0, percentage: 0, map ID: 0, map status: 0, has map: 0, map state: 0, is host: 0, bIsConnectedOnUSB: 0, has new fw: 0, is unpaired: 0, last update time: 0, error code: -1, indicator event: 0, indicator value: 0
```

（この見本の起動の部分は、行を抜いてあり、スレッド番号は 1 つにそろえた。実物では、行ごとにスレッド番号が違う。）

プロセスの終了:

```
[2026-11-26 01:53:04.807 +9 0x3410 INFO OETServer::SetDongleToPowerSavingMode] [13328] Set dongle into standby.
[2026-11-26 01:53:05.013 +9 0x3410 INFO OETServer::Release] [13328] Release end
[2026-11-26 01:53:05.013 +9 0x3410 INFO ] [13328] -------------------- Log End --------------------
```

## 2. 日付の切り替わり

古い日のファイル（`ViveTrackerServer_20261126.txt`）の最後の記録は、次の日の日付になっている。

```
[2026-11-27 00:01:03.039 +9 0x3240 INFO OETServer::OnPeripheralDataChanged] [12864] Peripheral data changed - is charging: 0, battery percent: 74, system button: 0, grip: 0, trigger: 0, trackpad: 0, menu: 0
```

新しい日のファイル（`ViveTrackerServer_20261127.txt`）の先頭:

```
# Log rotate by date at 2026-11-27 00:01:03.40
[2026-11-27 00:01:03.045 +9 0x3240 INFO OETServer::DeviceStatusToVHServer] [12864] display index: 2, device index: 4, connected: 1, tracking status: 2, charging: 0, percentage: 74, map ID: 0, map status: 6, has map: 1, map state: 2, is host: 0, bIsConnectedOnUSB: 0, has new fw: 0, is unpaired: 0, last update time: 0, error code: -1, indicator event: 0, indicator value: 0
[2026-11-27 00:01:03.045 +9 0x3240 INFO OETServer::CheckHostIsStarted] [12864] Host running state:2 to 2
[2026-11-27 00:01:03.045 +9 0x3240 INFO OETServer::GetSetupStatus] [12864] Scan status: 1, Cali status: 1
[2026-11-27 00:01:03.045 +9 0x3240 INFO OETServer::ServiceStatusToVHServer] [12864] [ServiceStatusToVHServer] dongle connected: 1, tracker connection type: 0, scan status: 1, device counts: 3, cali status: 1, error code: 0, indicator event: 0, indicator value: 0, camera fps: 50
```

（古いファイルの最後の記録の本文は、同じ形の別の行で代えてある。時刻は実物。）

## 3. 「Wi-Fi 情報なし」の経過（2026-11-27 22:53。サーバーのファイル）

オーナーがペアリングをし直した直後。22:53:25.377〜22:53:35.000 の 283 行から、状態の繰り返しなどを省いた 36 行。流れは次のとおり。

1. トラッカー 2 がつながる（ホスト。状態の行が `is host: 1`、`Start host tracker.`）。
2. 22:53:26.257 に `Got host wifi info.`。
3. トラッカー 4 がつながり、Wi-Fi の情報を渡され（`Assign wifi info ...`）、ホストにつながる（`is connected to host: 1`）。
4. 22:53:32.961 に、**トラッカー 2 へ、もう一度 `switch to HOST mode.`**。このあと `Got host wifi info.` は出ない。
5. トラッカー 0 がつながる。22:53:33.658 から **`Wifi info is not yet available.`** が続く。

```
[2026-11-27 22:53:25.377 +9 0xF88 INFO OETServer::AddTrackerCB] [3976] Add new tracker: (2), connection type: 1
[2026-11-27 22:53:25.380 +9 0xF88 INFO OETServer::DeviceStatusToVHServer] [3976] display index: 0, device index: 2, connected: 1, tracking status: 5, charging: 0, percentage: 98, map ID: -1, map status: -1, has map: 1, map state: 0, is host: 1, bIsConnectedOnUSB: 0, has new fw: 0, is unpaired: 0, last update time: 0, error code: -1, indicator event: 1, indicator value: 1
（中略: 7 行）
[2026-11-27 22:53:25.382 +9 0x82B0 INFO OETServer::CreateTracker] [33456] Create tracker (2) - power state: 11, data format: 1, host running state: 0
（中略: 1 行）
[2026-11-27 22:53:25.382 +9 0x82B0 INFO OETServer::StartTracker] [33456] Start tracker - power state :21, data format: 1
（中略: 5 行）
[2026-11-27 22:53:25.383 +9 0xF88 INFO Tracker::UpdateSwitchingMode] [3976] power state: 0, is host: 1
[2026-11-27 22:53:25.383 +9 0xF88 INFO Tracker::UpdateSwitchingMode] [3976] Tracker (2) Switching mode: OFF
[2026-11-27 22:53:25.383 +9 0xF88 INFO Tracker::SetSpaceCalibrationInfo] [3976] Transform - rotation:(0.000000, 0.123456, 0.000000, 0.123456 w), pos:(-0.123456, 0.123456, -0.123456), scale: 0.123456
[2026-11-27 22:53:25.383 +9 0x82B0 INFO Tracker::UpdateSwitchingMode] [33456] power state: 21, is host: 1
（中略: 5 行）
[2026-11-27 22:53:25.384 +9 0x2850 INFO OETServer::OnPipeQueryData] [10320] (2) Set SteamVR index: 5
（中略: 9 行）
[2026-11-27 22:53:25.385 +9 0x82B0 INFO OETServer::CreateTracker] [33456] Start host tracker. ret: 0
[2026-11-27 22:53:25.385 +9 0x82B0 INFO OETServer::GetDeviceInfoAndResponse] [33456] Response - cmd type: 214, cmd key: (AAAA), device index: 2, result: 0, sn: (00XX000000001), model name: (VIVE Ultimate Tracker 1), fw version: (1.0.999.178), role: (0), device type: 0, force update: 0, type: 1
（中略: 3 行）
[2026-11-27 22:53:25.399 +9 0x82B0 ERROR OETServer::CreateDeviceAndResponse] [33456] Fail to create tracker. Device index: 2, tracking mode: 1, tracking format: 11, err: 22
（中略: 9 行）
[2026-11-27 22:53:26.257 +9 0xF88 INFO OETServer::ProcessDeviceMsg] [3976] Got host wifi info.
[2026-11-27 22:53:27.127 +9 0x497C INFO Tracker::CopyFromDriverThreadProc] [18812] (SteamVR: 5)(2)[49]Pose status is changed: Pose data - status: 3, pos: (0.000000, 0.000000, 0.000000), rotation: (0.123456, -0.123456, -0.123456, w 0.123456), vel: (0.000000, 0.000000, -0.000000), angular vel: (0.123456, -0.123456, -0.123456) 
（中略: 11 行）
[2026-11-27 22:53:28.522 +9 0xF88 ERROR OETServer::DeviceInfoCB] [3976] TRACKER NOT FOUND. tracker: 00XX000000002
（中略: 17 行）
[2026-11-27 22:53:28.857 +9 0xF88 INFO OETServer::AddTrackerCB] [3976] Add new tracker: (4), connection type: 1
（中略: 14 行）
[2026-11-27 22:53:28.863 +9 0x82B0 INFO Tracker::UpdateSwitchingMode] [33456] power state: 11, is host: 0
（中略: 39 行）
[2026-11-27 22:53:29.282 +9 0xF88 INFO OETServer::ProcessDeviceMsg] [3976] Assign wifi info to client tracker (index: 4).
（中略: 8 行）
[2026-11-27 22:53:30.018 +9 0xF88 INFO OETServer::ProcessDeviceMsg] [3976] Client tracker (index: 4) is connected to host: 1
（中略: 4 行）
[2026-11-27 22:53:30.643 +9 0x9A94 INFO Tracker::CopyFromDriverThreadProc] [39572] (SteamVR: 7)(4)[41]Pose status is changed: Pose data - status: 3, pos: (0.000000, 0.000000, 0.000000), rotation: (0.123456, -0.123456, 0.123456, w 0.123456), vel: (0.000000, -0.000000, 0.000000), angular vel: (-0.123456, -0.123456, 0.123456) 
（中略: 4 行）
[2026-11-27 22:53:31.503 +9 0x497C INFO Tracker::CopyFromDriverThreadProc] [18812] (SteamVR: 5)(2)[78]Pose status is changed: Pose data - status: 2, pos: (0.123456, -0.123456, -0.123456), rotation: (-0.123456, -0.123456, -0.123456, w 0.123456), vel: (-0.123456, 0.123456, -0.123456), angular vel: (-0.123456, -0.123456, -0.123456) 
（中略: 5 行）
[2026-11-27 22:53:32.961 +9 0xF88 INFO Tracker::UpdateSwitchingMode] [3976] Tracker (2) - switch to HOST mode.
[2026-11-27 22:53:32.961 +9 0xF88 INFO Tracker::UpdateSwitchingMode] [3976] Tracker (2) Switching mode: ON
（中略: 19 行）
[2026-11-27 22:53:33.244 +9 0xF88 INFO OETServer::AddTrackerCB] [3976] Add new tracker: (0), connection type: 1
（中略: 54 行）
[2026-11-27 22:53:33.658 +9 0xF88 WARN OETServer::ProcessDeviceMsg] [3976] Wifi info is not yet available.
（中略: 12 行）
[2026-11-27 22:53:34.283 +9 0xF88 WARN OETServer::ProcessDeviceMsg] [3976] Wifi info is not yet available.
（中略: 18 行）
[2026-11-27 22:53:34.362 +9 0x9A94 INFO Tracker::CopyFromDriverThreadProc] [39572] (SteamVR: 7)(4)[244]Pose status is changed: Pose data - status: 2, pos: (0.123456, -0.123456, -0.123456), rotation: (-0.123456, 0.123456, -0.123456, w 0.123456), vel: (-0.123456, -0.123456, 0.123456), angular vel: (0.123456, 0.123456, -0.123456) 
（中略: 4 行）
[2026-11-27 22:53:34.643 +9 0x82B0 INFO OETServer::PairTracker] [33456] Pair tracker, isPair: 0, device index:-2
[2026-11-27 22:53:34.643 +9 0x82B0 INFO OETServer::CancelPairing] [33456] Cancel pairing on dongle.
[2026-11-27 22:53:34.837 +9 0xF88 WARN OETServer::ProcessDeviceMsg] [3976] Wifi info is not yet available.
[2026-11-27 22:53:34.867 +9 0x440C INFO Tracker::CopyFromDriverThreadProc] [17420] (SteamVR: 6)(0)[36]Pose status is changed: Pose data - status: 3, pos: (0.000000, 0.000000, 0.000000), rotation: (0.123456, -0.123456, 0.123456, w 0.123456), vel: (0.000000, 0.000000, -0.000000), angular vel: (0.123456, -0.123456, 0.123456) 
（中略: 4 行）
```

このあと 23:01 まで、`Wifi info is not yet available.` が 671 件続いた（1 分あたり 29〜108 件）。そのあいだに省いた行の多くは、状態の行（`display index: ...`）、`Host running state:2 to 2`、`Scan status: 1, Cali status: 1`、`[ServiceStatusToVHServer] ...`、`Tracker is not found`（`WARN`）、`TRACKER NOT FOUND. tracker: <シリアル番号>`（`ERROR`）の繰り返し。

## 4. そのほかの種類（サーバー）

```
[2026-11-27 22:52:59.298 +9 0xF88 INFO OETServer::RemoveTrackerCB] [3976] Remove tracker (2), connection type: 1, power state:11 
[2026-11-27 22:52:59.300 +9 0xF88 INFO OETServer::WakeupDongle] [3976] Wake up dongle: (2).
[2026-11-27 22:52:59.301 +9 0xF88 INFO OETServer::RemoveTrackerCB] [3976] Remove tracker (2), connection type: 1, power state:11 
[2026-11-27 22:53:02.275 +9 0x82B0 INFO OETServer::PairTracker] [33456] Unpair All
[2026-11-27 22:53:02.313 +9 0x82B0 INFO OETServer::PairTracker] [33456] Pair tracker, isPair: 0, device index:0
[2026-11-27 22:48:09.047 +9 0xCF80 WARN Tracker::CopyFromDriverThreadProc] [53120] (SteamVR: -1)(0)(tracking status: 0) Position (-nan, -nan, -nan) includes NAN.
[2026-11-27 22:48:09.047 +9 0xCF80 WARN Tracker::CopyFromDriverThreadProc] [53120] (SteamVR: -1)(0)(tracking status: 0) Rotation (-nan, -nan, -nan, -nan) includes NAN.
[2026-11-27 22:52:43.463 +9 0xF88 WARN OETServer::OnTrackerNotificationMsg] [3976] Tracker is not found
```

次の行は、形だけの見本（時刻とスレッド番号は、上の行から借りた。数は例）。

```
[2026-11-27 22:53:34.900 +9 0xF88 INFO Tracker::UpdateSwitchingMode] [3976] Tracker (2) - switch to CLIENT mode.
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::TrackerSwitchAndResponse] [3976] Query wifi from tracker 2
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::ProcessDeviceMsg] [3976] Client tracker (index: 4) is connected to host: 0
[2026-11-27 22:53:34.900 +9 0xF88 ERROR OETServer::OnTrackerErrorNotification] [3976] OET LOST TRACKING! index: 2
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::OnTrackerErrorNotification] [3976] OET reloc back. index: 2
[2026-11-27 22:53:34.900 +9 0xF88 ERROR OETServer::OnTrackerStatusIndicatorUpdate] [3976] [2] OET OVERSPEED!
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::OnTrackerErrorNotification] [3976] Tracker 3 entered sleeping (lost tracking)
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::OnDongleRemovedCB] [3976] Dongle is removed.
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::OnDongleRemovedCB] [3976] dongle count: 0, tracker count: 0
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::OnSteamVrRunningChanged] [3976] SteamVR Running is changed to 1
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::OnPipeConnected] [3976] Client 0000000000000001 is connected
[2026-11-27 22:53:34.900 +9 0xF88 ERROR OETServer::OnPipeClientError] [3976] OnPipeClientError::OnPipeError (0000000000000001) : WriteFile Fail : 232
[2026-11-27 22:53:34.900 +9 0xF88 INFO OETServer::SaveCalibratedDataToFile] [3976] Save map creation time: Wed Nov 25 21:00:00 2026\n.
[2026-11-27 22:53:34.900 +9 0xF88 ERROR BiCollector::GetPcSerialNumber] [3976] 
```

## 5. ドライバーとドングル

ドライバー（`ViveTrackerDriver_20261127.txt`）:

```
[2026-11-27 19:07:27.229 +9 0xBC9C INFO HIDManager::OnMyTrackerArrival] [48284] Dongle (0000000000000002) is added
[2026-11-27 19:07:28.939 +9 0x3410 INFO TrackerDongleHID::DataDispatchThreadProc] [13328] Wait for HID data got timeout.\n
[2026-11-27 22:47:49.307 +9 0x3410 INFO TrackerDongleHID::AddTracker] [13328] Tracker is paired to slot 3, connected: 1
[2026-11-27 22:47:50.091 +9 0x82B0 INFO TrackerOnDongle::SetPowerState] [33456] (3) set power state: 21
[2026-11-27 22:47:51.316 +9 0x3410 INFO HIDManager::RemoveTrackerCB] [13328] Remove tracker. slot num: 3, type: 1
[2026-11-27 22:52:59.298 +9 0xF88 INFO TrackerDongleHID::ParseIndication] [3976] (2) Tracker is going to power-off.
```

最後の行（電源オフの知らせ）は、4 章の先頭の `Remove tracker (2), ...`（サーバー。同じ 22:52:59.298）と同じ出来事。

ドングル（`ViveTrackerDongle_20261127.txt`）:

```
[2026-11-27 00:00:06.739 +9 0x813C INFO dumpPUMRFDropRate] [33084]   - D[1][RSSI: -83 ~ -52][CH: 42]DropRate: 1.50 % (15/1000), MAX RF-LOST:  2 InvalidFmt (   0,   0,   0,   0) Cont.InvalidFmt+Drop:   2, Total DropRate: 1.50 % (15/1000) Recv(0,985)
[2026-11-27 00:09:21.459 +9 0x813C INFO dumpPUMRFQueryStatus] [33084] ===================================================================>
[2026-11-27 22:47:49.306 +9 0x3410 INFO dumpPUMRFConnect] [13328]   - Change Status:    Connected, bt mac address: (#3)(00 11 22 33 44 55)
[2026-11-27 22:47:51.300 +9 0x3410 INFO dumpPUMRFConnect] [13328]   - Change Status: Disconnected, bt mac address: (#3)(00 11 22 33 44 55)
```

（`Disconnected` の行の時刻は、例。）
