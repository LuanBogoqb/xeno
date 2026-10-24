# Roadmap

| Marco | Entrega |
|---|---|
| M0 ✔ | Dumps do RGH: flash, KV, fuses, CPU key (backup duplo feito) |
| M1 | Cadeia de boot decifrada e `kernel_17559.bin` reconstruído ✔ (ver `research/flash/`). Interpretador PPC (inteiros, SPR, HV) roda o CB_A até o handoff para o CB_B ✔ (`runcb`, ver `research/cb/`). Falta: CB_B em diante (CD, CE, CF/CG), VMX e MMU |
| M2 | Hypervisor: security engine, memória protegida, fuses; XOSC |
| M3 | `xboxkrnl` real + SMC |
| M4 | Southbridge: SATA/XTAF, USB HID, ethernet |
| M5 | GPU Xenos (command processor do Xenia como referência) |
| M6 | `xam` + dash; carregar Proto e GoldSpoof (teste: HUD do Proto desenha) |
| M7 | Live em bridge; resposta XOSC válida |

O console RGH real é o oráculo: mesmo teste nos dois lados, comparar registradores e memória.
