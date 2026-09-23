param(
    [ValidateSet('x86_64', 'aarch64')]
    [string]$Architecture = 'x86_64'
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
$runtime = Join-Path $root "target/cutout-runtime-$Architecture"
$revision = 'da9b5e364c465de65c49d91e696cd6485270757f'
if (!(Test-Path $runtime)) {
    git clone --depth 1 --branch v1.28.0 --recursive --shallow-submodules https://github.com/microsoft/onnxruntime.git $runtime
}
if ((git -C $runtime rev-parse HEAD) -ne $revision) {
    throw 'Unexpected ONNX Runtime source revision; use a separate build directory.'
}
$parallel = if ($env:CMAKE_BUILD_PARALLEL_LEVEL) { $env:CMAKE_BUILD_PARALLEL_LEVEL } else { '4' }
$gitRoot = Split-Path (Split-Path (Get-Command git).Source -Parent) -Parent
$patch = Join-Path $gitRoot 'usr/bin/patch.exe'
if (!(Test-Path $patch)) { throw "Git for Windows patch executable not found: $patch" }
$buildArgs = @(
    'tools/ci_build/build.py', '--build_dir', 'build/Windows', '--config', 'Release',
    '--update', '--build', '--parallel', $parallel, '--skip_tests', '--compile_no_warning_as_error',
    '--enable_msvc_static_runtime', '--cmake_extra_defines',
    'onnxruntime_ENABLE_CPUINFO=ON', 'onnxruntime_BUILD_UNIT_TESTS=OFF', "Patch_EXECUTABLE=$patch"
)
if ($Architecture -eq 'aarch64') { $buildArgs += '--arm64' }
Push-Location $runtime
try {
    python @buildArgs
    # The providers reference re2, which nothing in the default target ever builds.
    cmake --build build/Windows/Release/_deps/re2-build --config Release --target re2 --parallel $parallel
} finally {
    Pop-Location
}

# ort links one library when it finds one and otherwise enumerates each component itself, which
# already misses what this version splits out. Hand it the single library instead.
$vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
$studio = & $vswhere -latest -products * -property installationPath
Import-Module (Join-Path $studio 'Common7/Tools/Microsoft.VisualStudio.DevShell.dll')
$machine = if ($Architecture -eq 'aarch64') { 'ARM64' } else { 'X64' }
Enter-VsDevShell -VsInstallPath $studio -SkipAutomaticLocation `
    -DevCmdArguments "-arch=$($machine.ToLower()) -no_logo"

$output = Join-Path $runtime 'lib'
Remove-Item -Recurse -Force $output -ErrorAction Ignore
New-Item -ItemType Directory -Path $output | Out-Null
$arguments = Join-Path $output 'merge.rsp'
@("/OUT:`"$(Join-Path $output 'onnxruntime.lib')`"", "/MACHINE:$machine") +
    (Get-ChildItem (Join-Path $runtime 'build/Windows/Release') -Recurse -Filter '*.lib' |
        ForEach-Object { '"{0}"' -f $_.FullName }) | Set-Content -Path $arguments
lib.exe "@$arguments"
Remove-Item $arguments

$env:ORT_LIB_PATH = $output
if ($env:GITHUB_ENV) { "ORT_LIB_PATH=$env:ORT_LIB_PATH" >> $env:GITHUB_ENV }
