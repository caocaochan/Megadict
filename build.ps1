# Assumes PowerShell 7 and stable Rust with the Windows MSVC toolchain.
[CmdletBinding()]
param(
    [switch]$Refresh,
    [string]$Revision = ([DateTime]::UtcNow.ToString('yyyy.MM.dd.HHmmss'))
)

$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    $manifest = '.cache/resolved.json'
    if ($Refresh -or !(Test-Path -LiteralPath $manifest)) { & ./fetch.ps1 }
    $sources = Get-Content -Raw -LiteralPath $manifest | ConvertFrom-Json
    cargo run --release --locked -- $manifest dist/Megadict.zip $Revision
    if ($LASTEXITCODE -ne 0) { throw 'Megadict build failed.' }

    $archive = [IO.Compression.ZipFile]::OpenRead((Join-Path $PSScriptRoot 'dist/Megadict.zip'))
    try {
        foreach ($name in 'index.json', 'sources.json') {
            $destination = Join-Path $PSScriptRoot "dist/$name"
            [IO.Compression.ZipFileExtensions]::ExtractToFile($archive.GetEntry($name), $destination, $true)
        }
    } finally {
        $archive.Dispose()
    }

    $frequencyDir = 'dist/frequencies'
    $null = New-Item -ItemType Directory -Force $frequencyDir
    foreach ($source in $sources | Where-Object group -eq 'frequencies') {
        Copy-Item -LiteralPath $source.path -Destination (Join-Path $frequencyDir $source.asset) -Force
    }
} finally {
    Pop-Location
}
