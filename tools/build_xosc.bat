@echo off
REM Build xosc_capture.c into xosc_capture.xex with the Xbox 360 XDK.
REM XEDK must contain bin\win32\cl.exe, include\xbox and lib\xbox.
setlocal
set "CANDIDATE="
if exist "C:\Users\Luan Bogo\xdk\bin\win32\cl.exe" set "CANDIDATE=C:\Users\Luan Bogo\xdk"
if not defined CANDIDATE if exist "%XEDK%\bin\win32\cl.exe" set "CANDIDATE=%XEDK%"
if not defined CANDIDATE if exist "C:\PROGRA~2\MICROS~4\bin\win32\cl.exe" set "CANDIDATE=C:\PROGRA~2\MICROS~4"
if not defined CANDIDATE goto :nodk
set "XEDK=%CANDIDATE%"
set INCLUDE=%XEDK%\include\win32;%XEDK%\include\xbox;%XEDK%\include\xbox\sys
set LIB=%XEDK%\lib\win32;%XEDK%\lib\xbox
set PATH=%XEDK%\bin\win32;%PATH%
cd /d "%~dp0"
echo Using XDK: %XEDK%
"%XEDK%\bin\win32\cl.exe" /nologo /c /O2 /D_XBOX /Fo"%~dp0xosc_capture.obj" "%~dp0xosc_capture.c"
if errorlevel 1 exit /b 1
"%XEDK%\bin\win32\link.exe" /NOLOGO /OUT:"%~dp0xosc_capture.xex" /MACHINE:PPCBE /SUBSYSTEM:XBOX /XEX "%~dp0xosc_capture.obj" xboxkrnl.lib xapilib.lib xnet.lib xbdm.lib
if errorlevel 1 exit /b 1
echo Built tools\xosc_capture.xex
endlocal
exit /b 0

:nodk
echo cl.exe not found.
echo Checked C:\Users\Luan Bogo\xdk and XEDK=%XEDK%
echo Set XEDK to a directory containing bin\win32\cl.exe.
endlocal
exit /b 1
