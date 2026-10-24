# xb-emu

Emulador de sistema completo do Xbox 360 (LLE): boot real, dash original, Live via plugins no kernel patcheado.
Alvo: ISO bootável (Linux por baixo) que age como um 360 de verdade.

Decisão de escopo: [docs/decisions.md](docs/decisions.md). Roadmap: [docs/roadmap.md](docs/roadmap.md).

## Layout

| Pasta | Conteúdo |
|---|---|
| `docs/` | decisões e roadmap |
| `research/` | engenharia reversa de plugins e do console (um README por alvo) |
| `tools/` | scripts Python de análise (XEX2 → PE → disassembly) |
| `samples/` | binários originais, **fora do git** (hashes nos READMEs de `research/`) |

## Ferramentas

Requer Python 3 com `cryptography`; `capstone` só para `ppcdis.py`.

```
python tools/xexinfo.py     <x.xex>            # headers, imports, formato
python tools/xexextract.py  <x.xex> <out.img>  # decripta (AES) + descomprime (LZX) -> imagem PE
python tools/ppcimports.py  <x.xex>            # imports resolvidos por nome (slot/thunk)
python tools/ppcdis.py      <x.xex>            # disassembly PPC com chamadas nomeadas
python tools/bootchain.py   <flash.bin> <outdir>  # decifra CB_A/CB_B/CD/CE/CF e descomprime o kernel base
```

## Estado

- [x] Pesquisa dos plugins Proto e GoldSpoof17559 (ver `research/`)
- [x] M0: dumps do RGH (flash, KV, fuses, CPU key) em `dumps/` (fora do git)
- [~] M1: cadeia de boot decifrada e kernel base extraído (ver `research/flash/`); kernel 17559 reconstruído e verificado; falta o emulador de CPU
