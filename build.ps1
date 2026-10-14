# Builds Wedit and its installer.
#   .\build.ps1          -> dist\wedit.exe and dist\Wedit-Setup-<version>.exe
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

Write-Host "==> Generating icon" -ForegroundColor Yellow
cargo run --release -p icon-gen
if ($LASTEXITCODE) { exit $LASTEXITCODE }

Write-Host "==> Running tests" -ForegroundColor Yellow
cargo test --release -p wedit
if ($LASTEXITCODE) { exit $LASTEXITCODE }

Write-Host "==> Building Wedit" -ForegroundColor Yellow
cargo build --release -p wedit
if ($LASTEXITCODE) { exit $LASTEXITCODE }

Write-Host "==> Building installer" -ForegroundColor Yellow
cargo build --release -p wedit-installer
if ($LASTEXITCODE) { exit $LASTEXITCODE }

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"(.+)"').Matches[0].Groups[1].Value
New-Item -ItemType Directory -Force dist | Out-Null
Copy-Item target\release\wedit.exe dist\wedit.exe -Force
Copy-Item target\release\wedit-setup.exe "dist\Wedit-Setup-$version.exe" -Force
Copy-Item assets\wedit.ico dist\wedit.ico -Force

Write-Host "==> Done" -ForegroundColor Green
Get-ChildItem dist | Format-Table Name, @{n="Size (MB)"; e={[math]::Round($_.Length / 1MB, 2)}}
