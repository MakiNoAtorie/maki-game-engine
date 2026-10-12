@echo off
rem Builds and runs the editor. Arguments go to the editor: the project folder (not
rem given: game), then the config file. Example: editor.bat game my.toml
cd /d "%~dp0"
if not defined MAKI_LOG set MAKI_LOG=info
rem One build command for the editor and for the script DLL of the project
rem (project.toml, `scripts`). The editor builds the project with the same command.
cargo build --profile dynamic -p maki-editor-app -p maki-game-dll
if errorlevel 1 goto failed
rem The editor links the DLL of the standard library: it is in the folder of the
rem compiler, not beside the program.
for /f "delims=" %%i in ('rustc --print sysroot') do set "PATH=%%i\lib\rustlib\x86_64-pc-windows-msvc\lib;%PATH%"
target\dynamic\maki-editor.exe %*
if errorlevel 1 goto failed
exit /b 0
:failed
rem Keep the window open after an error when started by double-click.
pause
