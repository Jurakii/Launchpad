@echo off
cd /d "%~dp0"

echo Installing dependencies...
call npm install
if errorlevel 1 (
    echo.
    echo npm install failed.
    pause
    exit /b 1
)

echo.
echo Building Launchpad...
call npm run dist

if errorlevel 1 (
    echo.
    echo Installer build hit an error ^(see above^) - this usually means the
    echo NSIS installer step, which needs Windows Developer Mode turned on
    echo ^(Settings -^> Privacy ^& Security -^> For developers^).
    echo A standalone app should still be sitting in dist\win-unpacked though -
    echo check for "Launchpad.exe" there.
) else (
    echo.
    echo Build succeeded. Installer and standalone app are in the dist folder.
)

pause
