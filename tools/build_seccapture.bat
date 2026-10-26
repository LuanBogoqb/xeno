@echo off
setlocal
set "CANDIDATE="
if exist "C:\Users\Luan Bogo\xdk\bin\win32\cl.exe" set "CANDIDATE=C:\Users\Luan Bogo\xdk"
if not defined CANDIDATE if exist "%XEDK%\bin\win32\cl.exe" set "CANDIDATE=%XEDK%"
if not defined CANDIDATE goto :nodk
set "XEDK=%CANDIDATE%"
set INCLUDE=%XEDK%\include\xbox;%XEDK%\include\win32;%XEDK%\include\xbox\sys
set LIB=%XEDK%\lib\xbox;%XEDK%\lib\win32
set PATH=%XEDK%\bin\win32;%PATH%
set BINARYPATH=%XEDK%\bin\win32
cd /d "%~dp0"
echo Using XDK: %XEDK%
if not exist build mkdir build
"%BINARYPATH%\cl.exe" /c /Zi /nologo /W3 /WX- /Ox /Os /D NDEBUG /D _XBOX /GF /Gm- /MT /GS- /Gy /fp:fast /fp:except- /Zc:wchar_t /Zc:forScope /GR- /openmp- /Fo"build/" /Fd"build/" /TP seccapture.cpp
if errorlevel 1 exit /b 1
"%BINARYPATH%\link.exe" /ERRORREPORT:PROMPT /OUT:"build/seccapture.exe" /INCREMENTAL:NO /NOLOGO xapilib.lib xboxkrnl.lib /MANIFESTUAC:"level='asInvoker' uiAccess='false'" /DEBUG /PDB:"build/seccapture.pdb" /STACK:"262144","262144" /OPT:REF /OPT:ICF /TLBID:1 /RELEASE /IMPLIB:"build/seccapture.lib" build/seccapture.obj /ignore:4089 /dll /entry:"_DllMainCRTStartup" /ALIGN:128,4096 /XEX:NO
if errorlevel 1 exit /b 1
"%BINARYPATH%\imagexex.exe" /nologo /config:"seccapture_xex.xml" /out:"SecCapture.xex" "build/seccapture.exe"
if errorlevel 1 exit /b 1
echo Built tools\SecCapture.xex
endlocal
exit /b 0

:nodk
echo cl.exe not found. Set XEDK to a directory containing bin\win32\cl.exe.
endlocal
exit /b 1
