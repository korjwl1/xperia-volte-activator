# EfsTools 셀프 컨테인드 빌드 — vendor/efstools/ 재현 스크립트
# 고정: 업스트림 master a8172de (v0.14 코드 + .NET 8 마이그레이션 커밋)
# 산출: EfsTools.dll SHA-256 = 99201687F99D462C0A3D3F718709250F9A9C345E9B1081C2C015B83A602D04FD
# 조사 근거: tasks/research-efstools-integration.md (라이선스·버전 확정분)
# 사용: powershell -ExecutionPolicy Bypass -File scripts\build-efstools.ps1

$ErrorActionPreference = "Stop"
$repo = "https://github.com/JohnBel/EfsTools.git"
$commit = "a8172de"   # master (2025-07-28 "Migrate to dotnet 8.0")
$outDir = Join-Path $PSScriptRoot "..\vendor\efstools"
$work = Join-Path $env:TEMP "efstools-build-repro"

# .NET 8 SDK — 없으면 사용자 로컬에 설치(관리자 불필요)
$dotnet8 = Join-Path $env:LOCALAPPDATA "Microsoft\dotnet\dotnet.exe"
if (-not (Test-Path $dotnet8)) {
    Write-Output "SDK 8 없음 — dotnet-install로 사용자 로컬 설치"
    Invoke-WebRequest -Uri "https://dot.net/v1/dotnet-install.ps1" -OutFile "$env:TEMP\dotnet-install.ps1"
    powershell -ExecutionPolicy Bypass -File "$env:TEMP\dotnet-install.ps1" -Channel 8.0 -InstallDir "$env:LOCALAPPDATA\Microsoft\dotnet"
}

# 소스 — 고정 커밋 체크아웃
if (Test-Path "$work\src") { Remove-Item "$work\src" -Recurse -Force }
git clone --no-checkout $repo "$work\src"
git -C "$work\src" checkout $commit

# 게시 — self-contained win-x64 (트림 없음: CommandLineParser 리플렉션 호환)
& $dotnet8 publish "$work\src\EfsTools\EfsTools.csproj" -c Release -f net8.0 -r win-x64 --self-contained true -o $outDir
if ($LASTEXITCODE -ne 0) { throw "publish 실패" }

# 무결성 — 산출 해시 기록·검증
$hash = (Get-FileHash "$outDir\EfsTools.dll" -Algorithm SHA256).Hash
Write-Output "EfsTools.dll SHA-256: $hash"
$expected = "99201687F99D462C0A3D3F718709250F9A9C345E9B1081C2C015B83A602D04FD"
if ($hash -ne $expected) {
    Write-Output "주의: 기록된 해시와 다름 — 업스트림 의존(NuGet) 갱신 가능성. 새 해시를 scripts/build-efstools.ps1과 설계문서에 반영할 것"
}
Write-Output "완료: $outDir"
