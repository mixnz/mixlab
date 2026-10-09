# One command installs MixLab on Windows: it picks the installer for this machine, checks its
# checksum, its signature and the name the signature vouches for, and runs it, per-user, with no
# administrator prompt.
#
#   irm https://mixnz.github.io/mixlab/install.ps1 | iex
#   & ([scriptblock]::Create((irm https://mixnz.github.io/mixlab/install.ps1))) -Headless -Version 0.0.13
#
# Roadmap task T197.
# Design: docs/specs/2026-10-05-t197-one-command-installs-mixlab-design.md
#
# Served from `master`, not from a release; see install.sh beside it for why. Functions only, and
# the call on the last line: a download cut off halfway defines functions and runs nothing.

function Get-MixLabConfig {
    [ordered]@{
        Releases     = 'https://github.com/mixnz/mixlab/releases'
        # packaging/updates.pub, line 2. .github/scripts/test-install.ps1 fails when they differ.
        PubKey       = 'RWSELKuybM79fmLhYywhXv8mdDvB3LPCC3TyHTdm2svTti6clLkck051'
        Oldest       = '0.0.8'
        Handbook     = 'https://mixnz.github.io/mixlab/en/install/'
        MinisignUrl  = 'https://github.com/jedisct1/minisign/releases/download/0.12/minisign-0.12-win64.zip'
        MinisignHash = '37b600344e20c19314b2e82813db2bfdcc408b77b876f7727889dbd46d539479'
    }
}

function Write-MixLabLine([string]$Message) {
    Write-Host "mixlab-install: $Message"
}

function ConvertTo-MixLabVersion([string]$Raw) {
    $v = $Raw -replace '^v', ''
    if ($v -notmatch '^\d+\.\d+\.\d+$') { throw "$Raw is not a version. use one such as 0.0.13" }
    $v
}

function Get-MixLabArch([string]$Raw) {
    switch ($Raw) {
        'X64' { 'x86_64' }
        'Arm64' { 'aarch64' }
        '' { throw "could not tell which processor this windows runs on. download the installer from $((Get-MixLabConfig).Releases)/latest" }
        default { throw "there is no build for $Raw windows" }
    }
}

function ConvertFrom-MixLabProcessorArch([string]$Native, [string]$Process) {
    # PROCESSOR_ARCHITEW6432 is set only in a 32-bit process on 64-bit windows, and names the
    # machine. An x64 process says AMD64 even on ARM: only windows 11 runs one there, and it runs
    # the x64 build too. Windows 10 on ARM emulates x86-32 alone, and its 32-bit process says ARM64
    # through PROCESSOR_ARCHITEW6432 (docs/decisions/0023-an-arm64-windows-machine-runs-the-x86_64-build.md).
    $raw = if ($Native) { $Native } else { $Process }
    switch ($raw) {
        'AMD64' { 'X64' }
        'ARM64' { 'Arm64' }
        'x86' { 'X86' }
        default { $raw }
    }
}

function Get-MixLabMachineArch {
    # .NET before 4.7.1 either has no RuntimeInformation, or finds the type and answers nothing for
    # OSArchitecture without an error, so an empty answer falls through to the environment too.
    $fromRuntime = try { [string][System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture } catch { '' }
    if ($fromRuntime) { return $fromRuntime }
    ConvertFrom-MixLabProcessorArch -Native $env:PROCESSOR_ARCHITEW6432 -Process $env:PROCESSOR_ARCHITECTURE
}

function Get-MixLabArtifactName([string]$Arch, [string]$Flavour, [string]$Version) {
    $stem = if ($Flavour -eq 'headless') { 'mixengine' } else { 'mixlab' }
    $tail = if ($Flavour -eq 'headless') { "windows-$Arch-headless-setup.exe" } else { "windows-$Arch-setup.exe" }
    if ($Version) { "$stem-$Version-$tail" } else { "$stem-$tail" }
}

function Get-MixLabReleaseBase([string]$Version) {
    $releases = (Get-MixLabConfig).Releases
    if ($Version) { "$releases/download/v$Version" } else { "$releases/latest/download" }
}

function Test-MixLabSha256([string]$File, [string]$SumFile) {
    $expected = ((Get-Content -TotalCount 1 $SumFile) -split '\s+')[0].ToLowerInvariant()
    $actual = (Get-FileHash -Algorithm SHA256 $File).Hash.ToLowerInvariant()
    $expected -ne '' -and $expected -eq $actual
}

function Get-MixLabMinisign([string]$Arch, [string]$Work) {
    $found = Get-Command minisign -ErrorAction SilentlyContinue
    if ($found) { return $found.Source }
    $config = Get-MixLabConfig
    Write-MixLabLine 'minisign is not installed, so this run fetches minisign 0.12 and removes it afterwards'
    $zip = Join-Path $Work 'minisign.zip'
    Invoke-WebRequest -UseBasicParsing -Uri $config.MinisignUrl -OutFile $zip
    if ((Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant() -ne $config.MinisignHash) {
        throw 'the minisign download does not match its pinned checksum. run this again later'
    }
    Expand-Archive -Path $zip -DestinationPath (Join-Path $Work 'minisign') -Force
    Join-Path $Work "minisign/minisign-win64/$Arch/minisign.exe"
}

function Test-MixLabSignature([string]$Minisign, [string]$Key, [string]$File, [string]$Name, [string]$Version) {
    # minisign writes its refusal to stderr. Under -ErrorAction Stop, Windows PowerShell 5.1 (and
    # PowerShell 7 with $PSNativeCommandUseErrorActionPreference) would turn that into its own
    # NativeCommandError; the exit code is what decides here, and the message is ours.
    $ErrorActionPreference = 'Continue'
    $PSNativeCommandUseErrorActionPreference = $false
    $out = (& $Minisign -V -H -P $Key -m $File 2>&1 | ForEach-Object { "$_" }) -join "`n"
    $code = $LASTEXITCODE
    $ErrorActionPreference = 'Stop'
    if ($code -ne 0) { throw "the signature does not match: $out" }
    $comment = ($out -split "`n" | Where-Object { $_ -like 'Trusted comment: *' } | Select-Object -First 1)
    $parts = ($comment -replace '^Trusted comment: ', '').Trim() -split ' '
    if ($parts.Count -ne 3 -or $parts[0] -ne 'mixengine' -or $parts[2] -ne $Name) {
        throw "the signature vouches for '$comment', not for $Name"
    }
    if ($Version -and $parts[1] -ne $Version) { throw "the signature is for version $($parts[1]), not $Version" }
    $parts[1]
}

function Install-MixLab {
    [CmdletBinding()]
    param(
        [switch]$Headless,
        [string]$Version = '',
        [switch]$DryRun,
        [switch]$PrintNames,
        [switch]$Help
    )
    $ErrorActionPreference = 'Stop'
    $config = Get-MixLabConfig
    # PowerShell 3 has no Get-FileHash and 4 no Expand-Archive, and both would fail only after the
    # download. 2 never gets here: it has no irm.
    if ($PSVersionTable.PSVersion.Major -lt 5) {
        throw "this script needs windows powershell 5.1, which windows 10 and later have. download the installer from $($config.Releases)/latest"
    }
    if ($Help) {
        Write-Host @'
Installs MixLab, or with -Headless the command-line programs alone.

  irm https://mixnz.github.io/mixlab/install.ps1 | iex
  & ([scriptblock]::Create((irm https://mixnz.github.io/mixlab/install.ps1))) -Headless

  -Headless          the four command-line programs, without the MixLab window
  -Version X.Y.Z     that release instead of the newest
  -DryRun            say what would be downloaded and run, check the URLs, install nothing
'@
        return
    }
    if ($Version) { $Version = ConvertTo-MixLabVersion -Raw $Version }
    if ($PrintNames) {
        foreach ($flavour in 'window', 'headless') {
            foreach ($arch in 'x86_64', 'aarch64') { Get-MixLabArtifactName -Arch $arch -Flavour $flavour -Version $Version }
        }
        return
    }
    if ($Version -and [version]$Version -lt [version]$config.Oldest) {
        throw "this script installs $($config.Oldest) or newer. download $Version from $($config.Releases)/tag/v$Version"
    }

    if ($PSVersionTable.PSVersion.Major -ge 6 -and -not $IsWindows -and -not $DryRun) {
        throw 'this script is for windows. on macos and linux run: curl -fsSL https://mixnz.github.io/mixlab/install.sh | sh'
    }

    # GitHub needs TLS 1.2, which Windows PowerShell 5.1 does not always offer by default, and its
    # progress bar makes Invoke-WebRequest many times slower.
    if ($PSVersionTable.PSVersion.Major -lt 6) {
        [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    }
    $ProgressPreference = 'SilentlyContinue'

    $arch = Get-MixLabArch -Raw (Get-MixLabMachineArch)
    $flavour = if ($Headless) { 'headless' } else { 'window' }
    $name = Get-MixLabArtifactName -Arch $arch -Flavour $flavour -Version $Version
    $base = Get-MixLabReleaseBase -Version $Version

    if ($DryRun) {
        Write-MixLabLine "found windows $arch"
        $failed = $false
        $urls = @("$base/$name", "$base/$name.sha256", "$base/$name.minisig")
        if (-not (Get-Command minisign -ErrorAction SilentlyContinue)) { $urls += $config.MinisignUrl }
        foreach ($url in $urls) {
            Write-Output $url
            try { Invoke-WebRequest -UseBasicParsing -Method Head -Uri $url | Out-Null }
            catch { Write-MixLabLine "no answer from $url"; $failed = $true }
        }
        Write-Output "<downloaded $name> /S"
        if ($failed) { throw 'a URL did not answer' }
        return
    }

    $work = Join-Path ([IO.Path]::GetTempPath()) ("mixlab-install-" + [Guid]::NewGuid())
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
        Write-MixLabLine "downloading $name"
        foreach ($part in $name, "$name.sha256", "$name.minisig") {
            Invoke-WebRequest -UseBasicParsing -Uri "$base/$part" -OutFile (Join-Path $work $part)
        }
        $file = Join-Path $work $name
        if (-not (Test-MixLabSha256 -File $file -SumFile "$file.sha256")) {
            throw "$name does not match its checksum. run this again; if it keeps failing, the download is being changed on the way"
        }
        $minisign = Get-MixLabMinisign -Arch $arch -Work $work
        $signed = Test-MixLabSignature -Minisign $minisign -Key $config.PubKey -File $file -Name $name -Version $Version
        Write-MixLabLine "$name is version $signed, signed by MixLab"
        Write-MixLabLine "running: $name /S"
        $process = Start-Process -FilePath $file -ArgumentList '/S' -Wait -PassThru
        if ($process.ExitCode -ne 0) { throw "the installer exited with $($process.ExitCode)" }
        if ($Headless) {
            Write-MixLabLine 'installed. open a new terminal and run: mix status'
        } else {
            Write-MixLabLine 'installed. open MixLab from the Start menu, or in a new terminal run: mix status'
        }
    } finally {
        Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
    }
}

if ($env:MIXLAB_INSTALL_SOURCED -ne '1') { Install-MixLab @args }
