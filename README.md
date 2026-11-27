# OpenUT

VIVE Ultimate Tracker を Windows の SteamVR で安定して使うための、非公式のソフトウェアです。HTC、VIVE、Valve とは関係がなく、承認も支援も受けていません。

An unofficial companion and replacement runtime for VIVE Ultimate Trackers on Windows + SteamVR. Not affiliated with HTC or Valve. Documents are in Japanese.

## いまの状態

設計の段階です。動くプログラムはまだありません（2026-11-28 に開発を始めました）。

## 作るもの

VIVE Hub の代わりに、VIVE Ultimate Tracker を SteamVR で使うための PC 側のソフトウェアです。日々の利用、ペアリング、部屋の地図の作成を受け持ちます。ファームウェアは作りません。ファームウェアとドライバーの更新には、VIVE Hub を使います。

| 段階 | 作るもの |
|---|---|
| M1 見張り役 | VIVE Hub を使ったまま、そのログを読み、追跡が始まらない・途切れる理由と対処を、その場で知らせる |
| M2 ドングルの読み取り | VIVE Hub を止めた状態で、ドングルから状態と姿勢を読むだけの道具 |
| M3 日々の利用 | VIVE Hub なしで追跡を始め、SteamVR に渡す。トラッカー側の暴走とクラッシュを検知して立て直す |
| M4 ペアリング | トラッカーとドングルのペアリング |
| M5 地図の作成 | 部屋の地図の作成と作り直し |

不調のときのログを集め、個人の情報を置き換えてから送れるようにする予定です。

経緯と根拠は `docs/research/`、要件は `docs/product/PRD.md`、設計は `docs/design/` にあります。

## 開発の進め方

複数の AI エージェントが、役割を分けて開発しています（`AGENTS.md`、`docs/project/`）。Issue と PR は、その受け渡しに使っています。外からの Issue と PR は歓迎しますが、取り込み方はこちらで決めます。ログを貼るときは、シリアル番号などを消してください。

## 入れないもの

- HTC のバイナリ、ファームウェア、HTC の更新サーバーへの問い合わせに使う値
- ライセンスの表示がない他人のコード
- 利用者のログそのもの（シリアル番号、無線のアドレス、Wi-Fi の情報、PC の名前、利用者のフォルダー名を置き換えたものだけを入れる）

## 注意

ドングルやトラッカーに命令を送るソフトは、機器を壊す恐れがあります。このレポジトリのコードは無保証です。

## ライセンス

MIT License（`LICENSE`）
