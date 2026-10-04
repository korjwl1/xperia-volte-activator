param([Parameter(Mandatory=$true)][string]$BundleRoot)
$ErrorActionPreference = 'Stop'
$fixtureDir = Join-Path $PSScriptRoot '../src-tauri/src/efs/fixtures'
$sets = @()
foreach ($setName in @('SonyEFS','SonyEFS_perf')) {
    $root = Join-Path $BundleRoot ('util/'+$setName)
    $presets = Get-ChildItem -LiteralPath $root -Directory | ForEach-Object { Get-ChildItem -LiteralPath $_.FullName -Directory }
    foreach ($preset in $presets) {
        $items = @(Get-ChildItem -LiteralPath $preset.FullName -File -Recurse | Sort-Object FullName | ForEach-Object {
            $rel=$_.FullName.Substring($preset.FullName.Length+1).Replace('\','/')
            $item = [ordered]@{ name=$rel; size=$_.Length; sha256=(Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
            $item
        })
        $sets += [ordered]@{ set=$setName; folder=$preset.Name; entries=$items }
    }
}
$json = ConvertTo-Json -InputObject $sets -Depth 6
[System.IO.File]::WriteAllText((Join-Path $fixtureDir 'stock-presets.json'), $json+"`n", [System.Text.UTF8Encoding]::new($false))
