@echo off
rem Builds and runs the game: a static program with no editor and no DLL.
rem Arguments go to the game: run.bat my.toml
rem Debug log shows resize events. Set MAKI_LOG before the call to change the level.
cd /d "%~dp0"
if not defined MAKI_LOG set MAKI_LOG=debug
cargo run -p maki-game -- %*
rem Keep the window open after an error when started by double-click.
if errorlevel 1 pause
