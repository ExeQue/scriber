# Installs Scriber on Windows (x64).
#
#   irm https://raw.githubusercontent.com/ExeQue/scriber/main/install/install.ps1 | iex
#
# Environment:
#   $env:SCRIBER_VERSION = 'v0.1.0'     install this release instead of the latest
#                                       ('dev': the unstable build of the newest commit on main)
#   $env:SCRIBER_INSTALL_DIR = '<dir>'  where Scriber goes (default: %LOCALAPPDATA%\Programs\Scriber)
#   $env:SCRIBER_UNINSTALL = '1'        remove Scriber, its Start Menu shortcut and its PATH entry
#   $env:SCRIBER_DOWNLOAD_BASE = '<url>' download releases from <url>/<tag>/<asset> (mirrors and testing)
#
# Re-running the script upgrades an existing install. Uninstalling never touches your settings,
# projects or material library.
& {
    $ErrorActionPreference = 'Stop'
    $ProgressPreference = 'SilentlyContinue'
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

    $repo = 'ExeQue/scriber'
    $dir = if ($env:SCRIBER_INSTALL_DIR) { $env:SCRIBER_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\Scriber' }
    $base = if ($env:SCRIBER_DOWNLOAD_BASE) { $env:SCRIBER_DOWNLOAD_BASE } else { "https://github.com/$repo/releases/download" }
    $uninstall = $env:SCRIBER_UNINSTALL -eq '1'
    $version = $env:SCRIBER_VERSION
    # One-shot options: clear them so the next run in the same window is a plain install of the latest.
    Remove-Item Env:SCRIBER_UNINSTALL, Env:SCRIBER_VERSION -ErrorAction SilentlyContinue
    $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Scriber.lnk'
    $exe = Join-Path $dir 'scriber.exe'

    # Read and write the raw registry value, so entries such as %USERPROFILE%\bin stay unexpanded.
    $envKey = Get-Item 'HKCU:\Environment'
    $userPath = $envKey.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
    function Set-UserPath($parts) {
        Set-ItemProperty 'HKCU:\Environment' Path ($parts -join ';') -Type ExpandString
        # Setting any user variable broadcasts the change, so new terminals see the new PATH.
        [Environment]::SetEnvironmentVariable('SCRIBER_INSTALL_PATH_CHANGED', '1', 'User')
        [Environment]::SetEnvironmentVariable('SCRIBER_INSTALL_PATH_CHANGED', $null, 'User')
    }
    $pathParts = @(if ($userPath) { $userPath -split ';' | Where-Object { $_ } })
    $onPath = $pathParts | Where-Object { $_.TrimEnd('\') -ieq $dir.TrimEnd('\') }

    if ($uninstall) {
        foreach ($f in 'scriber.exe', 'scriber.exe.old', 'LICENSE', 'README.md') {
            Remove-Item (Join-Path $dir $f) -Force -ErrorAction SilentlyContinue
        }
        if ((Test-Path $dir) -and -not (Get-ChildItem $dir -Force)) { Remove-Item $dir }
        Remove-Item $shortcut -Force -ErrorAction SilentlyContinue
        if ($onPath) {
            $rest = $pathParts | Where-Object { $_.TrimEnd('\') -ine $dir.TrimEnd('\') }
            Set-UserPath @($rest)
        }
        Write-Host "Removed Scriber from $dir, its Start Menu shortcut and its PATH entry."
        Write-Host 'Your settings, projects and material library are left as they are.'
        return
    }

    # The x64 build also runs on Windows 11 on Arm, through emulation.
    if ($env:PROCESSOR_ARCHITECTURE -notin 'AMD64', 'ARM64') {
        throw "There is no prebuilt Scriber for $env:PROCESSOR_ARCHITECTURE Windows. Build it from source: https://exeque.github.io/scriber/guide/installation#build-from-source"
    }
    $target = 'x86_64-pc-windows-msvc'

    $tag = $version
    if (-not $tag) {
        $tag = (Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest").tag_name
        if (-not $tag) { throw 'Could not look up the latest release; set $env:SCRIBER_VERSION (e.g. v0.1.0) to pick one.' }
    }

    $asset = "scriber-$tag-$target.zip"
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("scriber-" + [Guid]::NewGuid())
    New-Item -ItemType Directory $tmp | Out-Null
    try {
        Write-Host "Downloading $asset"
        $zip = Join-Path $tmp $asset
        Invoke-WebRequest "$base/$tag/$asset" -OutFile $zip -UseBasicParsing
        Invoke-WebRequest "$base/$tag/$asset.sha256" -OutFile "$zip.sha256" -UseBasicParsing

        $expected = ((Get-Content "$zip.sha256" -Raw).Trim() -split '\s+')[0]
        $actual = (Get-FileHash $zip -Algorithm SHA256).Hash
        if (-not $expected -or $expected -ine $actual) {
            throw "Checksum mismatch for $asset (expected $expected, got $actual)."
        }

        $pkg = Join-Path $tmp 'pkg'
        Expand-Archive $zip -DestinationPath $pkg
        if (-not (Test-Path (Join-Path $pkg 'scriber.exe'))) { throw "$asset has no scriber.exe." }

        New-Item -ItemType Directory $dir -Force | Out-Null
        # A running Scriber locks its exe, but Windows lets it be renamed; the old copy goes next time.
        Remove-Item "$exe.old" -Force -ErrorAction SilentlyContinue
        if (Test-Path $exe) { Move-Item $exe "$exe.old" -Force }
        Copy-Item (Join-Path $pkg '*') $dir -Force
        Unblock-File $exe -ErrorAction SilentlyContinue
    } finally {
        Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
    }

    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($shortcut)
    $link.TargetPath = $exe
    $link.WorkingDirectory = $dir
    $link.Description = '2D CAD for laser cutters, made for AI agents'
    $link.Save()

    if (-not $onPath) {
        Set-UserPath (@($pathParts) + $dir)
        $env:Path = "$env:Path;$dir"
        Write-Host "Added $dir to your user PATH (new terminals pick it up)."
    }

    Write-Host "Installed Scriber $tag to $exe, with a Start Menu shortcut."
    Write-Host 'Scriber is not code-signed yet: if SmartScreen warns when you start it, choose More info, then Run anyway.'
    Write-Host "Next: run 'scriber agents add' to connect your AI agents (https://exeque.github.io/scriber/agents/connecting)."
}
