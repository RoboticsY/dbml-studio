# DBML Studio

DBML ファイルをタブで開き、ER 図とテキストのどちらからも編集できる Windows・macOS 向けデスクトップアプリです。オフラインで動作します。

## インストール

[最新リリース](https://github.com/RoboticsY/dbml-studio/releases/latest) から、お使いの OS に合ったインストーラーをダウンロードしてください。

- Windows x64 はセットアップファイルを実行し、スタートメニューから DBML Studio を起動します。
- macOS は DMG を開き、DBML Studio.app を Applications にドラッグします。Apple Silicon と Intel に対応しています。Apple の公証を受けていないため、初回起動時に macOS が起動を止めることがあります。[Apple の案内](https://support.apple.com/ja-jp/102445)

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


macOS のユニバーサル DMG の作成には Python 3 と両方の Rust ターゲットを使用します。

    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    ./installer/build-macos.sh

DMG は `dist` に出力されます。
