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
- [x] M1: interpretador PPC executa a cadeia até o handoff do CD em `0x100`; kernel 17559 e patches validados byte a byte (`research/cb/`)
- [~] M2: HV executa o kernel virtual; modo experimental de seis threads passa a barreira multicore e chega ao idle guest. SMC validado com respostas reais do console. MMU com page walker SDR1, invalidacao seletiva de TLB, PP0 estendido e HID6 (SPR 0x3F9, `DLP`/`LB`) implementado. Capturado do console um par plaintext/ciphertext real de 4 KiB do security engine. Algoritmo implementado em security.rs: AES-128 XEX + tweak Toeplitz/GF(2) (Gooby MemCrypto.py), verificado byte-a-byte contra Gooby para chave all-0x55 e **validado em hardware**: as chaves de DRAM do boot ficam na RAM do HV (`0x80000102_000100F0`) e decifram o par de 4 KiB byte a byte (`dumps/console/sec_keys_v25.txt`, teste `hv_dump_key_search`); XOSC/Live adiado
