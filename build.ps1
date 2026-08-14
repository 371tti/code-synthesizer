[CmdletBinding()]
param(
    [switch]$Dev,
    [switch]$Test,
    [switch]$Smoke,
    [switch]$Install
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Invoke-Checked {
    param(
        [Parameter(Mandatory = $true)][string]$File,
        [string[]]$Arguments = @()
    )

    & $File @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Command failed ($LASTEXITCODE): $File $($Arguments -join ' ')"
    }
}

function Require-Command {
    param([Parameter(Mandatory = $true)][string]$Name)
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required command was not found: $Name"
    }
}

$workspaceRoot = $PSScriptRoot
$uiDirectory = Join-Path $workspaceRoot 'ui'
$bundleDirectory = Join-Path $workspaceRoot 'target\bundled\Code Synthesizer.vst3'
$profile = if ($Dev) { 'debug' } else { 'release' }

Require-Command cargo
Require-Command npm.cmd

Push-Location $workspaceRoot
try {
    # A fresh clone needs dependencies once; later builds reuse node_modules.
    if (-not (Test-Path (Join-Path $uiDirectory 'node_modules'))) {
        Invoke-Checked npm.cmd @('ci', '--prefix', 'ui')
    }

    if ($Test) {
        Invoke-Checked cargo @('fmt', '--all')
        Invoke-Checked cargo @('test', '--workspace')
        Invoke-Checked cargo @('clippy', '--workspace', '--all-targets', '--', '-D', 'warnings')
    }

    $bundleArguments = @('xtask', 'bundle')
    if (-not $Dev) {
        $bundleArguments += '--release'
    }
    Invoke-Checked cargo $bundleArguments

    if (-not (Test-Path $bundleDirectory)) {
        throw "VST3 bundle was not generated: $bundleDirectory"
    }

    if ($Install) {
        $vst3Directory = Join-Path $env:LOCALAPPDATA 'Programs\Common\VST3'
        New-Item -ItemType Directory -Path $vst3Directory -Force | Out-Null
        Copy-Item -LiteralPath $bundleDirectory -Destination $vst3Directory -Recurse -Force
        Write-Host "Installed: $(Join-Path $vst3Directory 'Code Synthesizer.vst3')"
    }

    if ($Smoke) {
        Invoke-Checked cargo @('run', '-p', 'synth-ui', '--example', 'ui-smoke')
    }

    Write-Host "Built ($profile): $bundleDirectory"
    Write-Host 'Restart or rescan your DAW after installing a new bundle.'
}
finally {
    Pop-Location
}
