# VIVE Hub ログの匿名化

レポジトリの直下で、共有用と非公開用のフォルダーを分けて実行します。同じトラッカーを突き合わせるログは、1 回のコマンドにまとめて渡します。

```powershell
cargo run --locked -- anonymize --out-dir share-logs --map-dir private-logs "ViveTrackerServer_20261127.txt" "ViveTrackerDriver_20261127.txt" "ViveTrackerDongle_20261127.txt"
```

対象は、BOM 付き UTF-16LE の `ViveTrackerServer_YYYYMMDD.txt`、`ViveTrackerDriver_YYYYMMDD.txt`、`ViveTrackerDongle_YYYYMMDD.txt` です。出力も同じ名前と文字コードを使います。Windows では `COMPUTERNAME` から PC 名を取得します。別の PC で採ったログを処理するときは、採取元の PC 名を `--pc-name` で指定できます。

非公開フォルダーの `openut-anonymize-map.tsv` には元の値が含まれます。対応表は非公開のまま保管してください。ログを共有する前に、警告が示す記録番号と、出力に残る位置・姿勢・時刻などを確認してください。不明な書式の値は警告付きで元の行に残ることがあります。確認済みの元の値がどれかの出力に残っている場合は、全ファイルの書き出しを中止します。
