$SRC_DIR = $PWD.Path
$STAGE = [System.Guid]::NewGuid().ToString()

Set-Location $ENV:Temp
New-Item -Type Directory -Name $STAGE
Set-Location $STAGE

$ZIP = "$SRC_DIR\bbmod.zip"
$LAUNCHER_BUNDLE_ROOT = "$SRC_DIR\launcher\src-tauri\target\release\bundle"

Copy-Item "$SRC_DIR\$($Env:CONFIGURATION)\dinput8.dll" '.\'
Copy-Item "$SRC_DIR\$($Env:CONFIGURATION)\dinput8.pdb" '.\'
Copy-Item "$SRC_DIR\README.md" '.\'
Copy-Item "$SRC_DIR\CHANGELOG.md" '.\'
Copy-Item "$SRC_DIR\addons" '.\' -Recurse

7z a "$ZIP" *

Push-AppveyorArtifact "$ZIP"

if (Test-Path $LAUNCHER_BUNDLE_ROOT) {
    $LAUNCHER_ZIP = "$SRC_DIR\launcher-bundle.zip"
    7z a "$LAUNCHER_ZIP" "$LAUNCHER_BUNDLE_ROOT\*"
    Push-AppveyorArtifact "$LAUNCHER_ZIP"
}

Set-Location $SRC_DIR
