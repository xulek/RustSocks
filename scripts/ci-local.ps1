param(
    [switch]$Clean
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
Push-Location $RepoRoot

try {
    if (-not (Test-Path (Join-Path $RepoRoot "Cargo.toml"))) {
        throw "Cargo.toml not found in repository root: $RepoRoot"
    }

    Write-Host "Repository: $RepoRoot"
    Write-Host "Cargo: $(& cargo --version)"
    Write-Host "Rustc: $(& rustc --version)"

    if ($Clean) {
        Write-Host ""
        Write-Host "==> Cleaning Cargo build artifacts"
        & cargo clean
        if ($LASTEXITCODE -ne 0) {
            throw "cargo clean failed with exit code $LASTEXITCODE"
        }
    }

    function Invoke-CargoStep {
        param(
            [Parameter(Mandatory = $true)]
            [string]$Name,
            [Parameter(Mandatory = $true)]
            [string[]]$Arguments
        )

        Write-Host ""
        Write-Host "==> $Name"
        & cargo @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "$Name failed with exit code $LASTEXITCODE"
        }
    }

    Invoke-CargoStep "Formatting" @("fmt", "--all", "--", "--check")

    Invoke-CargoStep "Check default features" @("check", "--locked")
    Invoke-CargoStep "Check all features" @("check", "--locked", "--all-features")
    Invoke-CargoStep "Check no default features" @("check", "--locked", "--no-default-features")

    Invoke-CargoStep "Clippy default features" @("clippy", "--locked", "--", "-D", "warnings")
    Invoke-CargoStep "Clippy all features" @("clippy", "--locked", "--all-features", "--", "-D", "warnings")
    Invoke-CargoStep "Clippy no default features" @("clippy", "--locked", "--no-default-features", "--", "-D", "warnings")

    Invoke-CargoStep "Tests default features" @("test", "--locked", "--no-fail-fast")
    Invoke-CargoStep "Tests all features" @("test", "--locked", "--all-features", "--no-fail-fast")
    Invoke-CargoStep "Tests no default features" @("test", "--locked", "--no-default-features", "--no-fail-fast")

    Write-Host ""
    Write-Host "All Rust CI checks passed."
}
finally {
    Pop-Location
}
