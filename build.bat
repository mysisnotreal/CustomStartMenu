@echo off
rem Builds all three executables into dist\:
rem   CSM-cpp.exe       Custom Start, C++ backend
rem   CSM-rust.exe      Custom Start, Rust backend
rem   CSMInstaller.exe  Setup / updater
setlocal

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
    echo vswhere.exe not found. Install Visual Studio 2022 with "Desktop development with C++".
    exit /b 1
)
set "VSDIR="
"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath > "%TEMP%\csm_vsdir.txt"
set /p VSDIR=<"%TEMP%\csm_vsdir.txt"
del "%TEMP%\csm_vsdir.txt" >nul 2>&1
if not defined VSDIR (
    echo Visual C++ tools not found.
    exit /b 1
)
call "%VSDIR%\VC\Auxiliary\Build\vcvars64.bat" >nul 2>&1 || exit /b 1

cd /d "%~dp0"
if not exist build mkdir build
if not exist dist mkdir dist

set "CLFLAGS=/nologo /utf-8 /std:c++17 /O2 /EHsc /W3 /MT /DUNICODE /D_UNICODE"

echo [1/3] C++ backend
rc /nologo /fo build\app.res cpp\app.rc || exit /b 1
cl %CLFLAGS% cpp\main.cpp build\app.res /Fo"build\\" /Fe"dist\CSM-cpp.exe" /link /SUBSYSTEM:WINDOWS || exit /b 1

echo [2/3] Installer
rc /nologo /fo build\installer.res installer\installer.rc || exit /b 1
cl %CLFLAGS% installer\installer.cpp build\installer.res /Fo"build\\" /Fe"dist\CSMInstaller.exe" /link /SUBSYSTEM:WINDOWS || exit /b 1

echo [3/3] Rust backend
where cargo >nul 2>&1
if errorlevel 1 (
    echo cargo not found - skipping the Rust backend.
) else (
    pushd rust
    cargo build --release || (popd & exit /b 1)
    popd
    copy /y rust\target\release\csm.exe dist\CSM-rust.exe >nul || exit /b 1
)

echo.
echo Done. Output in dist\
dir /b dist
