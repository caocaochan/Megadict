# Assumes PowerShell 7 and public caocaochan GitHub releases.
[CmdletBinding()]
param([switch]$IfChanged)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$cache = Join-Path $PSScriptRoot '.cache'
$manifest = Get-Content -Raw (Join-Path $PSScriptRoot 'sources.json') | ConvertFrom-Json -AsHashtable
$null = New-Item -ItemType Directory -Force $cache
$headers = @{ Accept = 'application/vnd.github+json' }
if ($env:GH_TOKEN) { $headers.Authorization = "Bearer $env:GH_TOKEN" }

$releases = @{}
$sources = foreach ($group in 'dictionaries', 'frequencies') {
    foreach ($source in $manifest[$group]) {
        $repo = "caocaochan/$($source.repo)"
        if (!$releases.ContainsKey($repo)) {
            $releases[$repo] = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -Headers $headers
        }
        $release = $releases[$repo]
        $assets = @($release.assets | Where-Object name -CEQ $source.asset)
        if ($assets.Count -ne 1 -or $assets[0].digest -notmatch '^sha256:([0-9a-f]{64})$') {
            throw "Expected one SHA-256 release asset matching $($source.asset) in $repo/$($release.tag_name)."
        }
        $asset = $assets[0]
        $digest = $Matches[1]
        [pscustomobject]@{
            id = $source.id
            group = $group
            release = $release.html_url
            asset = $asset.name
            download = $asset.browser_download_url
            size = $asset.size
            sha256 = $digest
            path = ".cache/$($source.id)-$digest.zip"
            local = @($source.local | ForEach-Object { if ($_) { $_.Replace('{tag}', $release.tag_name) } })
        }
    }
}

if ($IfChanged) {
    $latest = Invoke-WebRequest 'https://api.github.com/repos/caocaochan/Megadict/releases/latest' `
        -Headers $headers -SkipHttpErrorCheck
    if ($latest.StatusCode -eq 200) {
        $release = $latest.Content | ConvertFrom-Json
        $published = $release.assets | Where-Object name -CEQ 'sources.json'
        if (!$published) { throw "Latest release $($release.tag_name) is missing sources.json." }
        $snapshot = (Invoke-WebRequest $published.browser_download_url).Content
        if ($snapshot -is [byte[]]) { $snapshot = [Text.Encoding]::UTF8.GetString($snapshot) }
        $previous = $snapshot | ConvertFrom-Json
        $current = @($sources | Where-Object group -eq 'dictionaries')
        $previousHashes = @($previous | ForEach-Object { "$($_.id):$($_.sha256)" }) -join "`n"
        $currentHashes = @($current | ForEach-Object { "$($_.id):$($_.sha256)" }) -join "`n"
        if ($previousHashes -ceq $currentHashes) {
            Write-Host 'All definition archive checksums match the latest Megadict release.'
            if ($env:GITHUB_OUTPUT) { 'changed=false' >> $env:GITHUB_OUTPUT }
            return
        }
    } elseif ($latest.StatusCode -ne 404) {
        throw "Could not check the latest Megadict release (HTTP $($latest.StatusCode))."
    }
}

$root = $PSScriptRoot
$completed = @($sources | ForEach-Object -ThrottleLimit 4 -Parallel {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    $source = $_
    $curl = if ($IsWindows) { 'curl.exe' } else { 'curl' }
    $workspaceRoot = $using:root
    $output = Join-Path $workspaceRoot $source.path
    function Test-Asset($path) {
        $file = Get-Item -LiteralPath $path -ErrorAction SilentlyContinue
        $file -and $file.Length -eq $source.size -and (Get-FileHash -LiteralPath $path).Hash -eq $source.sha256
    }
    if (Test-Asset $output) {
        Write-Host "Cached $($source.asset)"
        $source.id
        return
    }
    $reusable = $source.local | ForEach-Object { Join-Path $workspaceRoot $_ } |
        Where-Object { Test-Asset $_ } | Select-Object -First 1
    $temporary = "$output.$([guid]::NewGuid().ToString('N')).tmp"
    try {
        if ($reusable) {
            Copy-Item -LiteralPath $reusable -Destination $temporary
        } else {
            & $curl --fail --location --silent --show-error --retry 3 --output $temporary $source.download
            if ($LASTEXITCODE -ne 0) { throw "Download failed for $($source.download)." }
        }
        if (!(Test-Asset $temporary)) { throw "Release digest mismatch for $($source.asset)." }
        Move-Item -LiteralPath $temporary -Destination $output -Force
        Write-Host "Verified $($source.asset)"
        $source.id
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
    }
})
if ($completed.Count -ne $sources.Count) { throw 'Some release archives could not be fetched.' }

$sources | Select-Object id, group, release, asset, sha256, path |
    ConvertTo-Json -Depth 5 | Set-Content -Encoding utf8 (Join-Path $cache 'resolved.json')
if ($env:GITHUB_OUTPUT) { 'changed=true' >> $env:GITHUB_OUTPUT }
