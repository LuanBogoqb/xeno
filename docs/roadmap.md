# Roadmap

| Marco | Entrega |
|---|---|
| M0 ✔ | Dumps do RGH: flash, KV, fuses, CPU key (backup duplo feito) |
| M1 ✔ | Interpretador PPC executa CB_A → CB_B → CD, CE e CF/CG até o `rfid` final do CD para PC `0x100`; kernel-base e kernel 17559 conferidos byte a byte, patches da NAND e argumentos de boot validados. 271.049.701 passos CD/CF (271.049.695 antes da resposta SMC `0x01` real); fuses originais, sem override CD/CF. TLB/cache ainda experimentais (`research/cb/`). Execução do HV/kernel após o handoff, VMX e MMU completa ficam para M2 |
| M2 ~ | HV executa kernel virtual; modo experimental 6 threads passa barreira e chega ao idle. PP/NX, timebase/decrementer, reservas por thread. SMC `0x01/0x12/0x07/0x0A/0x0F/0x17` validado com respostas reais do console. MMU ganhou page walker SDR1 (hashed PTEG) e invalidação seletiva de TLB; PP0 encodings estendidos implementados; HID6 (SPR `0x3F9`) implementado com `DLP` (bit 20) e `LB` documentados. Cifra AES-128 XEX + tweak Toeplitz implementada em `security.rs`, verificada byte a byte contra `MemCrypto.py` e **validada em hardware**: as chaves de DRAM do boot ficam na RAM do HV (tabela de runtime em `0x80000102_000100F0`, `hash`/`white`/`aes`) e decifram o par capturado do console (4 KiB, 256 blocos) byte a byte — ver `dumps/console/sec_keys_v25.txt` e o teste `hv_dump_key_search` (detalhes em `research/hv/security_engine_research.md` §8/§9). XOSC/Live adiados |
| M3 | `xboxkrnl` real + SMC |
| M4 | Southbridge: SATA/XTAF, USB HID, ethernet |
| M5 | GPU Xenos (command processor do Xenia como referência) |
| M6 | `xam` + dash; carregar Proto e GoldSpoof (teste: HUD do Proto desenha) |
| M7 | Live em bridge; resposta XOSC válida |

O console RGH real é o oráculo: mesmo teste nos dois lados, comparar registradores e memória.
