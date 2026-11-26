$ErrorActionPreference = "Stop"
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    cargo build --release --locked
    if ($LASTEXITCODE) { throw "Cargo build failed" }
    $json = cargo metadata --format-version 1 --locked --filter-platform x86_64-pc-windows-msvc
    if ($LASTEXITCODE) { throw "Cargo metadata failed" }
    $metadata = ($json -join [Environment]::NewLine) | ConvertFrom-Json
    $product = $metadata.packages | Where-Object id -eq $metadata.resolve.root
    $version = $product.version
    $binary = Join-Path $metadata.target_directory "release/dbml-studio.exe"
    $dist = New-Item -ItemType Directory -Force dist
    $notices = Join-Path $dist.FullName "THIRD-PARTY-NOTICES.txt"
    $installer = Join-Path $dist.FullName "DBML-Studio-$version-windows-x64-setup.exe"
    $writer = [IO.StreamWriter]::new($notices, $false, [Text.UTF8Encoding]::new($false))
    try {
        foreach ($pkg in $metadata.packages | Sort-Object name, version) {
            if ($pkg.id -eq $product.id) { continue }
            $writer.WriteLine("$($pkg.name) $($pkg.version)  $($pkg.license)  $($pkg.repository)")
            $dir = Split-Path -Parent $pkg.manifest_path
            $files = Get-ChildItem -Path $dir -Recurse -File |
                Where-Object { $_.Name -match '^(LICENSE|LICENCE|COPYING|NOTICE|OFL|UFL)([-._].*)?$' -and $_.Extension -notin '.rs','.html','.pdf' }
            foreach ($f in $files) {
                $writer.WriteLine([IO.File]::ReadAllText($f.FullName))
            }
            $writer.WriteLine()
        }
    } finally { $writer.Dispose() }
    $icon = (Get-Item assets/app-icon.ico).FullName
    makensis /V2 "/DAPP_ICON=$icon" "/DVERSION=$version" "/DAPP_EXE=$binary" "/DNOTICES=$notices" "/DOUTPUT_FILE=$installer" installer/setup.nsi
    if ($LASTEXITCODE) { throw "NSIS build failed" }
    Write-Output $installer
} finally { Pop-Location }
