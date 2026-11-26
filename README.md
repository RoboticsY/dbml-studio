# DBML Studio

DBML ファイルをタブで開き、ER 図とテキストのどちらからも編集できる Windows・macOS 向けデスクトップアプリです。オフラインで動作します。

## インストール

[最新リリース](https://github.com/RoboticsY/dbml-studio/releases/latest) の **Assets** から、お使いの OS に合ったファイルをダウンロードします。

### Windows（x64）

1. ファイル名が `-windows-x64-setup.exe` で終わるセットアップファイルをダウンロードします。
2. ダウンロードしたファイルをダブルクリックします。セットアップ画面でインストール先を確認し、インストールします。既定のインストール先は `%LOCALAPPDATA%\Programs\DBML Studio` です。
3. インストールが完了したら、スタートメニューで「DBML Studio」を検索して起動します。

セットアップファイルは未署名のため、Windows が警告を表示する場合があります。リリースページに掲載している SHA-256 で、ダウンロードしたファイルを確認できます。

### macOS（Apple Silicon）

1. ファイル名が `-macos-arm64.dmg` で終わるディスクイメージをダウンロードします。
2. ダウンロードした DMG をダブルクリックし、表示されたウインドウで「DBML Studio.app」を「Applications」にドラッグします。
3. Finder の「アプリケーション」から「DBML Studio」を起動します。コピーが完了したら、Finder でディスクイメージを取り出せます。

macOS 版は Apple の公証を受けていません。初回起動がブロックされた場合は、公式リリースから取得したことを確認し、Apple の案内に沿って「システム設定」→「プライバシーとセキュリティ」で起動を許可できます。 [Apple の案内](https://support.apple.com/ja-jp/102445)

## 編集と自動保存

- 「開く」で複数の DBML ファイルを選べます。図のテーブル、カラム、参照線を選ぶと、右側に編集項目が表示されます。「DBML テキスト」でテキストエディタも開けます。
- 図のテーブル名、カラム名、データ型をダブルクリックすると、その場で編集できます。Enter で確定し、Esc で取り消します。
- 編集が約 1.2 秒止まると、変更したタブの内容を検査して整形し、自動保存します。別のタブへ移っても保存されます。新規文書の下書きは、自動保存後に画面下に表示されるフォルダー（Windows は `%LOCALAPPDATA%\DBML Studio\Drafts`、macOS は `~/Library/Application Support/DBML Studio/Drafts`）にあります。「保存」で名前と保存先を指定できます。
- テキスト入力中も DBML を検査します。エラーがある間は直前の図を表示し、ディスク上の DBML を保持します。修正すると自動保存が再開します。ファイルが外部で変更された場合は、内容を確認してから別名で保存してください。
- テーブルの見出しをドラッグすると配置が変わります。座標は同じフォルダーの `ファイル名.dbml.layout.json` に自動保存します。別のフォルダーや PC に図を移すときは、DBML と配置ファイルを一緒にコピーしてください。
- 図の PK は主キー、FK は外部キーを示します。列挙型や複合参照などの定義はテキストで編集できます。使用しているパーサーは `TablePartial` や `Records` など一部の構文に対応していません。

## ソースからビルド

Windows x64 では Rust の MSVC ツールチェーンと Visual Studio の C++ ビルドツール、macOS では Rust と Xcode Command Line Tools を使用します。

```powershell
cargo test --locked
cargo run --release --locked
```

Windows インストーラーの作成には NSIS 3 と PowerShell を使います。`makensis` を PATH に追加したうえで、リポジトリのルートから実行してください。

```powershell
./installer/build.ps1
```

セットアップファイルは `dist` に出力されます。


Apple Silicon 向けの DMG の作成には Python 3 と Rust の arm64 ターゲットを使用します。

    rustup target add aarch64-apple-darwin
    ./installer/build-macos.sh

DMG は `dist` に出力されます。
