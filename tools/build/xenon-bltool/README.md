Build do [xenon-bltool](https://github.com/InvoxiPlayGames/xenon-bltool) (GPL-2) com MSVC, para conferir `tools/bootchain.py`.
O fonte do xenon-bltool não é copiado para este repo; são só os shims:

1. `gh repo clone InvoxiPlayGames/xenon-bltool` e entre na pasta.
2. Copie `shim.h` e `build.bat` para a raiz e `fp61.h` por cima de `3rdparty/excrypt/fp61.h` (troca `__int128` por `_umul128`; `shim.h` cobre `strcasecmp` e `__builtin_ffs`).
3. Rode `build.bat` (ajuste o caminho do `vcvars64.bat`). Saída: `xenon-bltool.exe`.

Limites: `nand_extract` não entende a imagem RGH3 (CB_A2 / CB_B em claro) e o `xboxupd` não lê o CG daqui (offset de `cg_hmac` e lacuna em `0xC0000`).
