# T197: the Windows install script's choices, checked without an installer. Runs on Windows
# PowerShell 5.1 and on PowerShell 7, including pwsh on Linux for the parts that are not Windows'.
# Helper names use approved verbs, because PSScriptAnalyzer checks this file too.
$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '../..')
$env:MIXLAB_INSTALL_SOURCED = '1'
. (Join-Path $root 'packaging/install/install.ps1')

$script:failures = 0
function Assert-Same([string]$What, $Expected, $Actual) {
    if ("$Expected" -ne "$Actual") {
        Write-Host "FAIL $What`n  expected: $Expected`n  actual:   $Actual"
        $script:failures++
    }
}
function Test-Refused([scriptblock]$Block) {
    try { & $Block | Out-Null; 'accepted' } catch { 'refused' }
}

$pub = (Get-Content (Join-Path $root 'packaging/updates.pub'))[1].Trim()
Assert-Same 'the embedded key is packaging/updates.pub''s' $pub (Get-MixLabConfig).PubKey

Assert-Same 'a plain version' '0.0.14' (ConvertTo-MixLabVersion -Raw '0.0.14')
Assert-Same 'a tag' '0.0.14' (ConvertTo-MixLabVersion -Raw 'v0.0.14')
Assert-Same 'two numbers' 'refused' (Test-Refused { ConvertTo-MixLabVersion -Raw '0.14' })

Assert-Same 'X64' 'x86_64' (Get-MixLabArch -Raw 'X64')
Assert-Same 'Arm64' 'aarch64' (Get-MixLabArch -Raw 'Arm64')
Assert-Same 'X86 is refused' 'refused' (Test-Refused { Get-MixLabArch -Raw 'X86' })

Assert-Same 'window' 'mixlab-windows-x86_64-setup.exe' (Get-MixLabArtifactName -Arch x86_64 -Flavour window -Version '')
Assert-Same 'headless arm' 'mixengine-windows-aarch64-headless-setup.exe' (Get-MixLabArtifactName -Arch aarch64 -Flavour headless -Version '')
Assert-Same 'window, versioned' 'mixlab-0.0.14-windows-x86_64-setup.exe' (Get-MixLabArtifactName -Arch x86_64 -Flavour window -Version '0.0.14')
Assert-Same 'headless, versioned' 'mixengine-0.0.14-windows-aarch64-headless-setup.exe' (Get-MixLabArtifactName -Arch aarch64 -Flavour headless -Version '0.0.14')

Assert-Same 'latest' 'https://github.com/mixnz/mixlab/releases/latest/download' (Get-MixLabReleaseBase -Version '')
Assert-Same 'a version' 'https://github.com/mixnz/mixlab/releases/download/v0.0.14' (Get-MixLabReleaseBase -Version '0.0.14')

# -PrintNames through @args, the way `& ([scriptblock]::Create(...)) -PrintNames` passes it.
function Invoke-Relay { Install-MixLab @args }
Assert-Same 'switches survive @args' 4 (@(Invoke-Relay -PrintNames -Version '0.0.14')).Count

# Off Windows only a dry run makes sense: a real run would try to start setup.exe.
if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows) {
    $why = try { Install-MixLab -Headless | Out-Null; 'accepted' } catch { $_.Exception.Message }
    Assert-Same 'a real run off windows is refused before downloading' $true ($why -like '*is for windows*')
}

$work = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid())
New-Item -ItemType Directory -Path $work | Out-Null
try {
    'a package' | Set-Content -NoNewline (Join-Path $work 'x.exe')
    $hash = (Get-FileHash -Algorithm SHA256 (Join-Path $work 'x.exe')).Hash.ToLowerInvariant()
    "$hash  x.exe" | Set-Content (Join-Path $work 'x.exe.sha256')
    Assert-Same 'the checksum matches' $true (Test-MixLabSha256 -File (Join-Path $work 'x.exe') -SumFile (Join-Path $work 'x.exe.sha256'))
    'changed' | Add-Content (Join-Path $work 'x.exe')
    Assert-Same 'a changed file fails' $false (Test-MixLabSha256 -File (Join-Path $work 'x.exe') -SumFile (Join-Path $work 'x.exe.sha256'))
} finally {
    Remove-Item -Recurse -Force $work
}

if ($script:failures -ne 0) { throw "$($script:failures) check(s) failed" }
Write-Host 'install.ps1: every check passed'
