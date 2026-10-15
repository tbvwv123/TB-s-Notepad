# TB's Notepad 릴리스 빌드 스크립트
# 사용법:  powershell -ExecutionPolicy Bypass -File .\build.ps1
# 결과물:  dist\TB-Notepad.exe  (이 파일 하나만 있으면 어디서든 실행됩니다)

$ErrorActionPreference = "Stop"

# 새 터미널을 열기 전에도 cargo와 MinGW(dlltool)를 찾을 수 있게 PATH 보충
$extra = @("$env:USERPROFILE\.cargo\bin")
$mingw = Get-ChildItem "$env:LOCALAPPDATA\Microsoft\WinGet\Packages" -Directory -Filter "BrechtSanders.WinLibs*" -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($mingw) { $extra += (Join-Path $mingw.FullName "mingw64\bin") }
$env:PATH = ($extra -join ";") + ";" + $env:PATH

Set-Location $PSScriptRoot
cargo build --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$target = (cargo metadata --format-version 1 --no-deps | ConvertFrom-Json).target_directory
New-Item -ItemType Directory -Force "dist" | Out-Null
$exe = "dist\TB-Notepad.exe"
# 지난번에 비켜 둔 실행 파일은 (이제 꺼졌으면) 지웁니다. 아직 켜져 있는 것(MCP 서버 등)은 그대로 둡니다.
Get-ChildItem "dist" -Filter "TB-Notepad.old*.exe" -ErrorAction SilentlyContinue |
    ForEach-Object { Remove-Item $_.FullName -Force -ErrorAction SilentlyContinue }
$old = "dist\TB-Notepad.old.exe"
if (Test-Path $old) { $old = "dist\TB-Notepad.old-{0}.exe" -f (Get-Date -Format "yyyyMMdd-HHmmss") }
# 앱이 켜져 있으면 실행 파일이 잠겨 덮어쓸 수 없지만, 이름 바꾸기는 됩니다.
# 켜진 앱은 그대로 두고 새 파일을 넣습니다 (다음에 앱을 다시 켜면 새 버전).
$running = $false
if (Test-Path $exe) {
    try {
        [System.IO.File]::Open((Resolve-Path $exe), 'Open', 'ReadWrite', 'None').Close()
    } catch {
        Move-Item $exe $old -Force
        $running = $true
    }
}
Copy-Item (Join-Path $target "release\tb-notepad.exe") $exe -Force
$size = (Get-Item $exe).Length / 1MB
"완료: $exe ({0:N1} MB)" -f $size
if ($running) { "앱이 켜져 있어서 이전 실행 파일은 $old 로 비켜 두었습니다. 앱을 다시 켜면 새 버전입니다." }
