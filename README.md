# OpenUT

VIVE Ultimate Tracker を Windows の SteamVR で安定して使うための、非公式のソフトウェアです。HTC、VIVE、Valve とは関係がなく、承認も支援も受けていません。

An unofficial companion and replacement runtime for VIVE Ultimate Trackers on Windows + SteamVR. Not affiliated with HTC or Valve. Documents are in Japanese.

## いまの状態

設計の段階です。動くプログラムはまだありません。

## 作るもの

1. **見張り役（段階1）**: VIVE Hub のログを読み、追跡が始まらない・途切れる理由と対処を、その場で知らせる。VIVE Hub はそのまま使う。
2. **置き換えランタイム（段階2）**: VIVE Hub のトラッカー用サーバーの代わりにドングルを直接扱い、SteamVR にトラッカーとして渡す。地図の作成とファームウェアの更新は VIVE Hub に任せる。

経緯と根拠は `docs/research/`、要件は `docs/product/PRD.md`、設計は `docs/design/` にあります。

## 入れないもの

- HTC のバイナリ、ファームウェア、HTC の更新サーバーへの問い合わせに使う値
- ライセンスの表示がない他人のコード
- 利用者のログそのもの（シリアル番号と無線のアドレスを置き換えたものだけを入れる）

## 注意

ドングルやトラッカーに命令を送るソフトは、機器を壊す恐れがあります。このレポジトリのコードは無保証です。

## ライセンス

MIT License（`LICENSE`）
