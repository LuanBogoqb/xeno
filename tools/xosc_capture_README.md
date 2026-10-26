# xosc_capture.xex

Homebrew de captura para Xbox 360 (XDK 20353).

## Arquivo pronto

`tools/xosc_capture.xex` (102.400 bytes, load address `0x82000000`).

Ele é **sem assinatura** (RGH/dash launch necessário) e ainda usa um
**placeholder** no lugar da chamada XOSC real: grava o challenge e um
response marcado com `0xA5`. Serve para validar o caminho de captura
(TCP + arquivo) antes de ligar o serviço real do HV.

## O que ele faz

1. Monta um challenge de 20 bytes (nonce + flags).
2. Chama `xosc_exchange` (placeholder).
3. Consulta o SMC com o comando `0x01` via `HalSendSMCMessage` e grava
   `smc_01.bin` (4 bytes de status + 16 bytes de resposta).
4. Grava `challenge+response` (56 bytes) em `xosc_capture.bin`.
   O caminho preferido é `\??\game:\...` (ao lado do `.xex`), com fallback
   para USB/HDD.
5. Tenta enviar os mesmos 56 bytes por TCP para `PC_IP:9999`.

Ajuste `PC_IP` em `tools/xosc_capture.c` e recompile com
`tools/build_xosc.bat`.

## Compilar

```bat
tools\build_xosc.bat
```

O script usa `C:\Users\Luan Bogo\xdk` (árvore extraída do instalador
XDK) ou `C:\Program Files (x86)\Microsoft Xbox 360 SDK` se o `cl.exe`
estiver lá. Ele chama o compilador PowerPC do XDK e o linker com
`/SUBSYSTEM:XBOX /XEX`.

## Rodar no console

1. Copie `tools\xosc_capture.xex` para o console (USB/FTP).
2. No PC, escute: `nc -l 9999 > challenge_response.bin`.
3. Rode o `.xex` via XeLL/dash launch/Aurora.
4. Se a rede não funcionar, pegue o arquivo `xosc_capture.bin` do HDD/USB.

## Validar no emulador

```powershell
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage hv --xosc-test challenge_response.bin
```

O modo `--xosc-test` compara os bytes `[20..52]` do arquivo com o digest
do modelo. Com o placeholder atual o resultado será `MISMATCH`; isso é
esperado até a chamada XOSC real ser implementada.
