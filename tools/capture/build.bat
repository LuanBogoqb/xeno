@echo off
REM Build one of the console capture binaries with the Xbox 360 XDK.
REM
REM   tools\capture\build.bat plugin        SecCapture.xex   (Dashlaunch kernel plugin)
REM   tools\capture\build.bat capture_all   capture_all.xex  (user-mode memory dump)
REM   tools\capture\build.bat xosc          xosc_capture.xex (XOSC/SMC capture)
REM
REM The XDK is taken from %XEDK% or from the usual install paths.
setlocal enabledelayedexpansion

set "TARGET=%~1"
if "%TARGET%"=="" set "TARGET=plugin"

set "CANDIDATE="
if not defined CANDIDATE if defined XEDK if exist "%XEDK%\bin\win32\cl.exe" set "CANDIDATE=%XEDK%"
if not defined CANDIDATE if exist "C:\PROGRA~2\MICROS~4\bin\win32\cl.exe" set "CANDIDATE=C:\PROGRA~2\MICROS~4"
if not defined CANDIDATE goto :nodk

set "XEDK=%CANDIDATE%"
set "BIN=%XEDK%\bin\win32"
cd /d "%~dp0"
echo Using XDK: %XEDK%  (target: %TARGET%)

if /i "%TARGET%"=="plugin" goto :plugin
if /i "%TARGET%"=="capture_all" goto :capture_all
if /i "%TARGET%"=="xosc" goto :xosc
echo Unknown target "%TARGET%". Use plugin, capture_all or xosc.
exit /b 2

REM ------------------------------------------------------------------ plugin
:plugin
set INCLUDE=%XEDK%\include\xbox;%XEDK%\include\win32;%XEDK%\include\xbox\sys
set LIB=%XEDK%\lib\xbox;%XEDK%\lib\win32
set PATH=%BIN%;%PATH%
set BINARYPATH=%BIN%
if not exist build mkdir build
"%BIN%\cl.exe" /c /Zi /nologo /W3 /WX- /Ox /Os /D NDEBUG /D _XBOX /GF /Gm- /MT /GS- /Gy /fp:fast /fp:except- /Zc:wchar_t /Zc:forScope /GR- /openmp- /Fo"build/" /Fd"build/" /TP seccapture.cpp
if errorlevel 1 exit /b 1
"%BIN%\link.exe" /ERRORREPORT:PROMPT /OUT:"build/seccapture.exe" /INCREMENTAL:NO /NOLOGO xapilib.lib xboxkrnl.lib /MANIFESTUAC:"level='asInvoker' uiAccess='false'" /DEBUG /PDB:"build/seccapture.pdb" /STACK:"262144","262144" /OPT:REF /OPT:ICF /TLBID:1 /RELEASE /IMPLIB:"build/seccapture.lib" build/seccapture.obj /ignore:4089 /dll /entry:"_DllMainCRTStartup" /ALIGN:128,4096 /XEX:NO
if errorlevel 1 exit /b 1
"%BIN%\imagexex.exe" /nologo /config:"seccapture_xex.xml" /out:"SecCapture.xex" "build/seccapture.exe"
if errorlevel 1 exit /b 1
echo Built tools\capture\SecCapture.xex
exit /b 0

REM ------------------------------------------------------------- capture_all
:capture_all
set INCLUDE=%XEDK%\include\win32;%XEDK%\include\xbox;%XEDK%\include\xbox\sys
set LIB=%XEDK%\lib\win32;%XEDK%\lib\xbox
set PATH=%BIN%;%PATH%
"%BIN%\cl.exe" /nologo /c /O2 /D_XBOX /Fo"%~dp0capture_all.obj" "%~dp0capture_all.c"
if errorlevel 1 exit /b 1
"%BIN%\link.exe" /NOLOGO /OUT:"%~dp0capture_all.xex" /MACHINE:PPCBE /SUBSYSTEM:XBOX /XEX "%~dp0capture_all.obj" xboxkrnl.lib xapilib.lib xnet.lib xbdm.lib
if errorlevel 1 exit /b 1
echo Built tools\capture\capture_all.xex
exit /b 0

REM -------------------------------------------------------------------- xosc
:xosc
set INCLUDE=%XEDK%\include\win32;%XEDK%\include\xbox;%XEDK%\include\xbox\sys
set LIB=%XEDK%\lib\win32;%XEDK%\lib\xbox
set PATH=%BIN%;%PATH%
"%BIN%\cl.exe" /nologo /c /O2 /D_XBOX /Fo"%~dp0xosc_capture.obj" "%~dp0xosc_capture.c"
if errorlevel 1 exit /b 1
"%BIN%\link.exe" /NOLOGO /OUT:"%~dp0xosc_capture.xex" /MACHINE:PPCBE /SUBSYSTEM:XBOX /XEX "%~dp0xosc_capture.obj" xboxkrnl.lib xapilib.lib xnet.lib xbdm.lib
if errorlevel 1 exit /b 1
echo Built tools\capture\xosc_capture.xex
exit /b 0

:nodk
echo cl.exe not found. Set XEDK to a directory containing bin\win32\cl.exe.
exit /b 1
