# Flash do console (Corona 4GB eMMC, RGH3)

Dump: `dumps/FLASH_1.bin` (48 MiB, via XeLL web, 2 leituras idênticas). Sem ECC, offsets planos.
Ferramenta: `python tools/bootchain.py dumps/FLASH_1.bin dumps/stages` (saída em `dumps/stages/`, fora do git).

## Header (`0x00`)

`FF4F`, entry = CB em `0x8000`, `kv_size 0x4000`, `cf_offset 0xB0000`, 2 patch slots, `kv_version 0x0712`.

## Cadeia de boot

| Estágio | Offset | Build | Tamanho | Estado no dump | Chave RC4 |
|---|---|---|---|---|---|
| CB_A | `0x8000` | 10918 | `0x1AF0` | cifrado | `HMAC-SHA1(1BL key, salt@0x10)[:16]` |
| CB_A2 (zeropair, loader RGH3) | `0x9AF0` | 15432 | `0x400` | cifrado, salt zero | não derivada (mantido cru) |
| CB_B | `0x9EF0` | 13121 | `0x7B40` | **em claro** (o campo `key` já é a chave) | — |
| CD | `0x11A30` | 12905 | `0x5400` | cifrado | `HMAC(CB_B.key, CD.salt)` (sem CPU key quando há CB_B) |
| CE | `0x16E30` | 1888 | `0x5606A` | cifrado | `HMAC(chave do CD, CE.salt)` |
| CF | `0xB0000` | 17559 | `0x4560` | cifrado | `HMAC(1BL key, CF.key@0x20)`, decifra de `0x30` |
| CG | `0xB4560` | 17559 | `0x77A76` | cifrado | `HMAC(CF[0x330:0x340], CG.key@0x10)`, decifra de `0x20` |

Todas as chaves derivadas são `HMAC-SHA1(...)[:16]`, RC4 até o tamanho alinhado. A 1BL key `DD88AD0C9ED669E7B56794FB68563EFA` é pública. A **CPU key não foi necessária** nesta imagem.

## Kernel

- CE (LZX, janela 128 KiB, com tradução E8) → `kernel_base.bin`, 0x120000 bytes, build 1888.
- CG = patch LZX-delta (janela 32 KiB pré-preenchida com os bytes antigos) → `kernel_17559.bin`, 0x1F0000 bytes, magic `NN 44 97`.
- O CG **não é contíguo**: o bloco de 64 KiB em `0xC0000` é quase todo `0xFF` e deve ser pulado (o CG continua em `0xD0000`).

## SMC

O firmware do SMC fica em `0x800`; o header plano em `0x78` aponta tamanho e
offset. Neste dump: 14.336 bytes, versão **2.05**.

```powershell
python tools/smcextract.py dumps/FLASH_1.bin dumps/smc-firmware
```

Saída em `dumps/smc-firmware/` (fora do git):

| Arquivo | SHA-256 |
|---|---|
| `SMC_en.bin` (cifrado) | `1e908cd607c70cb1009be055e40051b228a28bc49f88cc81086a659a325bfc3a` |
| `SMC_dec.bin` (plano) | `492cd815a722d4e52e512d084453f9d0a663b10dd866016b44877aad0f50c143` |

O `manifest.json` guarda offset, tamanho, versão e o hash da fonte
(`FLASH_1.bin`, `37e5ce5b…40a7545`). A versão do firmware bate com a resposta
viva do comando `0x12` (`12 62 02 05…`, versão **2.05**) registrada em
`dumps/smc/SMC-00.TXT`.

É firmware lido da flash, não uma resposta do SMC em execução; patches do RGH
podem estar presentes.

## Validação

- `kernel_base.bin` é idêntico byte a byte ao `ce_unc` do xenon-bltool (compilado aqui, ver `tools/build/xenon-bltool/`) e o SHA-1 dele bate com `original_hash` do CG.
- `kernel_17559.bin` bate com `new_hash` do CG (SHA-1 `f1b66f2aa794f0117a8a2d565c9fec0b601fed83`).
- CB_A: o xenon-bltool reporta assinatura RSA válida. O dump dele só difere do nosso no campo `key` e na área da assinatura, que ele reescreve ao verificar.
- Bug achado na validação cruzada: a tradução E8 do LZX não estava implementada (a descompressão divergia em `0x121C`). Corrigido.

## Em aberto

- CB_A2 não decifrado (provavelmente irrelevante pro emulador; existe só pro glitch).
- O xenon-bltool não decifra o CF/CG desta imagem (offset do `cg_hmac` e lacuna em `0xC0000`); a nossa implementação é a que funciona.
- Layout do restante da flash (sistema de arquivos e KV em `0x4000`).

Referência de algoritmo: InvoxiPlayGames/xenon-bltool (GPL-2, lido só para derivar as chaves; o código aqui é próprio).
