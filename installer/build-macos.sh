#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

export MACOSX_DEPLOYMENT_TARGET=11.0
target=aarch64-apple-darwin
cargo build --release --locked --target "$target"

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
app="$stage/DBML Studio.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" dist
cargo metadata --format-version 1 --locked --filter-platform "$target" > "$stage/metadata.json"
version=$(python3 - "$stage" "$app" <<'PY'
import json
import plistlib
import re
import sys
from pathlib import Path

stage, app = map(Path, sys.argv[1:3])
metadata = json.loads((stage / "metadata.json").read_text())
root = metadata["resolve"]["root"]
packages = {p["id"]: p for p in metadata["packages"]}
version = packages.pop(root)["version"]
(stage / "target-directory").write_text(metadata["target_directory"])
with (app / "Contents/Info.plist").open("wb") as plist:
    plistlib.dump({
        "CFBundleDisplayName": "DBML Studio",
        "CFBundleName": "DBML Studio",
        "CFBundleExecutable": "dbml-studio",
        "CFBundleIdentifier": "com.roboticsy.dbml-studio",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version,
        "CFBundleVersion": version,
        "LSMinimumSystemVersion": "11.0",
        "NSHighResolutionCapable": True,
    }, plist)

license_name = re.compile(r"^(LICENSE|LICENCE|COPYING|NOTICE|OFL|UFL)([-._].*)?$", re.I)
with (app / "Contents/Resources/THIRD-PARTY-NOTICES.txt").open("w", encoding="utf-8") as notices:
    for pkg in sorted(packages.values(), key=lambda p: (p["name"], p["version"])):
        print(pkg["name"], pkg["version"], pkg["license"] or "", pkg["repository"] or "", file=notices)
        directory = Path(pkg["manifest_path"]).parent
        for path in sorted(directory.rglob("*")):
            if (path.is_file() and license_name.fullmatch(path.name)
                    and path.suffix.lower() not in {".rs", ".html", ".pdf"}):
                print(path.read_text(encoding="utf-8", errors="replace"), file=notices)
        print(file=notices)
print(version)
PY
)
target_dir=$(cat "$stage/target-directory")
install -m 755 "$target_dir/$target/release/dbml-studio" "$app/Contents/MacOS/dbml-studio"
codesign --force --sign - "$app"
codesign --verify --strict --all-architectures "$app"

image="$stage/image"
mkdir "$image"
mv "$app" "$image/"
ln -s /Applications "$image/Applications"
file="dist/DBML-Studio-$version-macos-arm64.dmg"
hdiutil create -ov -format UDZO -volname "DBML Studio" -srcfolder "$image" "$file"
printf '%s\n' "$file"
