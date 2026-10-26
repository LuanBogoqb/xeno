# Ferramentas

Duas famílias: **análise** (Python, roda no PC) e **captura** (XDK, roda no
console). A biblioteca compartilhada fica em `tools/xbolib/`; os scripts são
wrappers finos.

```powershell
python -m pip install cryptography capstone
```

## Análise — boot chain e flash

| Ferramenta | Uso | O que faz |
|---|---|---|
| `bootchain.py` | `bootchain.py <flash.bin> <outdir>` | decifra CB_A/CB_B/CD/CE e CF/CG, reconstrói `kernel_base.bin` e `kernel_17559.bin`, confere os hashes |
| `flashinfo.py` | `flashinfo.py <flash.bin> <cpu_key> [1bl_key]` | explorador: lista os estágios e testa chaves dadas, mostrando qual decifra melhor |

Detalhes de offsets/chaves por estágio: [`research/flash/README.md`](../research/flash/README.md).

## Análise — XEX2

| Ferramenta | Uso | O que faz |
|---|---|---|
| `xexinfo.py` | `xexinfo.py <x.xex>` | headers opcionais, security info, formato do arquivo base |
| `xexextract.py` | `xexextract.py <x.xex> <out.img>` | AES + LZX → imagem PE crua |
| `ppcimports.py` | `ppcimports.py <x.xex>` | resolve imports por nome (tabelas do Xenia, em `tools/data/`) |
| `ppcdis.py` | `ppcdis.py <x.xex>` | disassembly PPC com chamadas de import nomeadas |

## Análise — estágios e capturas

| Ferramenta | Uso | O que faz |
|---|---|---|
| `cbscan.py` | `cbscan.py <stage.bin> [unknown.txt]` | histograma de mnemônicos, constantes em registradores, branches absolutos, SPRs |
| `test_cbscan.py` | `python tools/test_cbscan.py` | teste do classificador de opcodes desconhecidos |
| `parse_seccap.py` | `parse_seccap.py [capture.bin]` | lê a captura do plugin, compara com a imagem retail e mostra os marcos do HV |

## Captura (console, XDK)

Fonte e formatos em [`tools/capture/`](../tools/capture/) e no guia
[capturing-from-console.md](capturing-from-console.md).

```powershell
tools\capture\build.bat plugin        # SecCapture.xex: plugin de kernel (Dashlaunch)
tools\capture\build.bat capture_all   # capture_all.xex: dump de memória em user mode
tools\capture\build.bat xosc          # xosc_capture.xex: challenge XOSC + SMC
```

| Arquivo | Papel |
|---|---|
| `seccapture.cpp` | plugin de kernel; copia RAM do HV/SoC via hypercall para `Hdd1:\Plugins\seccap.bin` |
| `capture_all.c` | título XDK que tenta os mesmos ranges em user mode (o que dá sem kernel) |
| `xosc_capture.c` | captura o challenge/resposta XOSC e o comando SMC `0x01` |

## Biblioteca compartilhada

`tools/xbolib/` concentra o que antes estava copiado entre scripts:

| Módulo | Conteúdo |
|---|---|
| `binary.py` | leitores big/little-endian e alinhamento |
| `crypto.py` | AES (ECB/CBC), RC4 e a derivação HMAC-SHA1 |
| `lzx.py` | LZX do 360 (frames de 32 KiB, sem reset, tradução E8, modo delta) |
| `xex.py` | parse do XEX2, extração, seções PE, imports e strings |
| `boot.py` | cadeia CB_A..CG e reconstrução do kernel |

## Referência externa

`tools/build/xenon-bltool/` traz só os headers de compatibilidade usados para
compilar o `xenon-bltool` original e comparar a saída (créditos no README de
lá). `tools/data/` são as tabelas ordinal→nome do Xenia (BSD-3).
