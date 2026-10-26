# Hypervisor do Xbox 360 (17559): security engine e criptografia

Binário: `dumps/console/hypervisor_17559.bin` (65536 bytes).
CPU key no offset 0x20: `5cb2...6f78 (redacted; ver dumps/console/)`.

## Método

- Desmontado com Capstone (`CS_ARCH_PPC`, `CS_MODE_64 | CS_MODE_BIG_ENDIAN`).
- Descida recursiva a partir do vetor de reset (0x100) mais as tabelas de
  despacho de hypercall (offsets de arquivo 0x80/0xae80), para alcançar código
  que não é alvo de branch direto.
- Busca por referências ao deslocamento 0x20, endereços altos e de MMIO do SoC,
  laços de 10 a 16 iterações e constantes (0x1b, 0x63, 0x9e3779b9). Não há
  S-box de AES nem constante de TEA: o AES é de hardware, não está neste HV.

## 1. Funções que referenciam o deslocamento 0x20 (vizinho da CPU key)

A cópia de bloco de 128 bytes em `0xe14` carrega e grava `0x20(r3)` /
`0x20(r4)` ao copiar 16 qwords. Outros pontos que tocam o deslocamento `0x20`
de uma estrutura:

| Endereço | Contexto |
|---|---|
| `0x202c` | `lwz r10, 0x20(r3)` comparado com `0x42440302` (checagem de dispositivo/revisão) |
| `0x21ac`, `0x21c4` | `ld r11, 0x20(r31)` (cópia de 16 bytes para buffer) |
| `0x3e08` | `ld r11, 0x20(r11)` (lista encadeada / entrada de tabela) |
| `0x6a18` | `std r29, 0x20(r11)` (grava em registro de 0x40 bytes) |
| `0x85d4`, `0x884c` | `std r10, 0x20(r11)` (save/restore de registrador do security engine, seção 3) |
| `0x8fd0`, `0x8fd8` | `std/ld r11, 0x20(r18)` (limpeza e polling de registrador do security engine) |
| `0xa9b8`, `0xa9cc` | `ld/std r6, 0x20(r4/r3)` (cópia genérica de 8 qwords) |

A própria CPU key (0x20..0x2f do header do HV) é consumida pela cópia em `0xe14`
e pelo setup do security engine da seção 3; nenhuma outra rotina lê os 16 bytes
crus diretamente.

## 2. Endereços altos e do SoC acessados

Além da memória baixa habitual em `0x8000_xxxx`, o HV programa I/O mapeada em
memória nestas bases:

- `0x8000020000000000`: base do SoC usada por `lis 2/4/6; bl 0x6a0`.
- `0x8000020000040000` (bloco de I/O `+0x6000/+0x6100`): usada em `0x3088`.
- `0x8000020000050000` (bloco de I/O `+0x6000..+0x6040`): usada por `0x84e0`.
- `0x8000020000060000` (bloco de I/O `+0x1a0`, `+0x1c8`): usada por `0x84e0`.
- `0x80000200ea000000`: IIC (controlador de interrupções), programado em `0xb510`
  (`stw` para `+0x1014`).
- O bloco documentado do security engine é `0x24000` relativo ao SoC; o HV chega
  nele pelo caminho de save/restore de `0x84e0`, não por um imediato `0x24000`.

## 3. Rotina candidata do security engine (cifra de memória): 0x84e0

Chamada de `0x8b40`, `0x9370` e `0xa458` (`bl 0x84e0`), com a rotina par em
`0x87a8` (restore). Salva os cinco registradores do security engine
(`0x6000..0x6040`) em um global em `0x6360(r2)`, zera, espera a conclusão e
depois restaura. É a rotina que configura o motor de cifra de memória no boot e
na troca de mundo.

Disassembly completo:

```
84e0: mflr r12
84e4: bl 0xa808
84e8: stdu r1, -0x80(r1)
84ec: mfspr r30, 0x131
84f0: lbz r11, 0x81(r30)
84f4: cmplwi cr6, r11, 0
84f8: beq cr6, 0x8500
84fc: bl 0x218
8500: cmplwi cr6, r3, 2
8504: ble cr6, 0x850c
8508: bl 0x218
850c: stb r3, 0x82(r30)
8510: sync
8514: li r11, 1
8518: stb r11, 0x81(r30)
851c: sync
8520: lis r3, 6
8524: ori r3, r3, 0x1000
8528: bl 0x6a0
852c: mr r29, r3
8530: lis r3, 5
8534: bl 0x6a0
8538: lbz r11, 0x80(r30)
853c: rldicr r10, r11, 0xc, 0x3f
8540: cmplwi cr6, r11, 0
8544: add r28, r10, r3
8548: bne cr6, 0x86f4
854c: addis r11, r30, 1
8550: li r10, 5
8554: addi r11, r11, -0x5f7f
8558: b 0x8560
855c: mr r31, r31
8560: lbz r9, 0(r11)
8564: cmplwi cr6, r9, 1
8568: bne cr6, 0x855c
856c: addi r10, r10, -1
8570: addi r11, r11, -0x2000
8574: cmplwi cr6, r10, 0
8578: bne cr6, 0x8560
857c: ld r11, 0x1a0(r29)
8580: li r12, -0x101
8584: and r11, r11, r12
8588: std r11, 0x1a0(r29)
858c: eieio
8590: ld r11, 0x1a0(r29)
8594: cmpldi cr6, r11, 0
8598: beq cr6, 0x85a0
859c: nop
85a0: isync
85a4: addis r11, r2, 1
85a8: ld r10, 0x6000(r3)
85ac: li r31, 0
85b0: addi r11, r11, 0x6360
85b4: std r10, 0(r11)
85b8: ld r10, 0x6010(r3)
85bc: std r10, 8(r11)
85c0: ld r10, 0x6020(r3)
85c4: std r10, 0x10(r11)
85c8: ld r10, 0x6030(r3)
85cc: std r10, 0x18(r11)
85d0: ld r10, 0x6040(r3)
85d4: std r10, 0x20(r11)
85d8: std r31, 0x6000(r3)
85dc: eieio
85e0: std r31, 0x6010(r3)
85e4: eieio
85e8: std r31, 0x6020(r3)
85ec: eieio
85f0: std r31, 0x6030(r3)
85f4: eieio
85f8: std r31, 0x6040(r3)
85fc: eieio
8600: ld r10, 0x6040(r3)
8604: cmpldi cr6, r10, 0
8608: beq cr6, 0x8610
860c: nop
8610: isync
8614: ld r10, 0x1c8(r29)
8618: std r10, 0x28(r11)
861c: std r31, 0x1c8(r29)
8620: eieio
8624: ld r11, 0x1c8(r29)
8628: cmpldi cr6, r11, 0
862c: beq cr6, 0x8634
8630: nop
8634: isync
8638: li r10, 5
863c: li r11, 0
8640: rldimi r11, r10, 0xd, 0x13
8644: add r11, r11, r30
8648: lbz r9, 0x82(r11)
864c: lbz r8, 0x82(r30)
8650: cmplw cr6, r9, r8
8654: bne cr6, 0x86ec
8658: li r9, 2
865c: stb r9, 0x81(r11)
8660: sync
8664: b 0x866c
8668: mr r31, r31
866c: lbz r9, 0x81(r11)
8670: cmplwi cr6, r9, 3
8674: bne cr6, 0x8668
8678: addi r10, r10, -1
867c: cmplwi cr6, r10, 0
8680: bne cr6, 0x863c
8684: bl 0x4cc
8688: li r11, 0x200
868c: mr r10, r31
8690: lbz r9, 0(r3)
8694: addi r11, r11, -1
8698: addi r3, r3, 0x80
869c: add r10, r9, r10
86a0: cmplwi cr6, r11, 0
86a4: bne cr6, 0x8690
86a8: cmpldi cr6, r10, 0
86ac: beq cr6, 0x86b4
86b0: nop
86b4: isync
86b8: li r11, 0x180
86bc: mr r10, r30
86c0: lbz r9, 0(r10)
86c4: addi r11, r11, -1
86c8: addi r10, r10, 0x80
86cc: add r31, r9, r31
86d0: cmplwi cr6, r11, 0
86d4: bne cr6, 0x86c0
86d8: cmpldi cr6, r31, 0
86dc: beq cr6, 0x86e4
86e0: nop
86e4: isync
86e8: b 0x8700
86ec: bl 0x218
86f0: mr r31, r31
86f4: lbz r11, 0x81(r30)
86f8: cmplwi cr6, r11, 2
86fc: bne cr6, 0x86f0
8700: li r11, 0x7c
8704: std r11, 8(r28)
8708: eieio
870c: ld r11, 8(r28)
8710: cmpldi cr6, r11, 0
8714: beq cr6, 0x871c
8718: nop
871c: isync
8720: mfspr r10, 0x16
8724: lis r11, 0x7fff
8728: stw r10, 0x88(r30)
872c: ori r11, r11, 0xffff
8730: mtspr 0x16, r11
8734: lbz r9, 0x80(r30)
8738: clrlwi r10, r9, 0x1f
873c: cmplwi cr6, r10, 0
8740: bne cr6, 0x8768
8744: b 0x874c
8748: mr r31, r31
874c: mfspr r10, 0x88
8750: rlwinm r10, r10, 0, 9, 9
8754: cmpldi cr6, r10, 0
8758: bne cr6, 0x8748
875c: mfspr r10, 0x136
8760: stw r10, 0x8c(r30)
8764: mtspr 0x136, r11
8768: mfspr r11, 0x399
876c: sync
8770: lis r12, -0x71
8774: nor r10, r9, r9
8778: ori r12, r12, 0xffff
877c: rlwinm r10, r10, 0x14, 0xb, 0xb
8780: and r11, r11, r12
8784: or r11, r10, r11
8788: mtspr 0x399, r11
878c: sync
8790: isync
8794: li r11, 3
8798: stb r11, 0x81(r30)
879c: sync
87a0: addi r1, r1, 0x80
87a4: b 0xa858
```

A rotina par de restore em `0x87a8` (trecho) recarrega os cinco valores salvos
de `0x6360(r2)` em `0x6000..0x6040(r3)` e faz polling até concluir.

## 4. Laços de 10 a 16 iterações

A varredura estática encontrou os laços `mtctr` abaixo, com contador entre 8 e
20. Nenhum contém constante de rodada de AES nem S-box: são cópia de bytes,
checksum e polling de MMIO, não criptografia em software.

| Endereço | Contador | Papel (pelo contexto) |
|---|---|---|
| `0x216c` | 16 | cópia de 16 bytes para buffer |
| `0x3cd4` | 16 | cópia de tabela |
| `0x3f34` | 16 | cópia de tabela |
| `0x502c` | 16 | cópia de tabela |
| `0x53c4` | 16 | cópia de tabela |
| `0x54d0` | 16 | cópia de tabela |
| `0x5aa0` | 16 | cópia de tabela |
| `0x5ae8` | 16 | cópia de tabela |
| `0x6ec8` | 10 | cópia de 10 bytes |
| `0x9788`, `0x9928`, `0x9a78`, `0x9d18`, `0xa088`, `0xa14c`, `0xa480` | 12 | cópia de 12 bytes por volta |
| `0x9f3c` | 16 | cópia de 16 bytes |
| família `0x2108` | 12 | preenchimento de buffer |
| `0x42a0`, `0x7104` | 20 | cópia maior |

## 5. Grafo de chamadas a partir do vetor de reset (0x100)

```
0x100 (reset do sistema)
  -> 0x1870 (init primária; só se SPR 0x131 == 0)
       0x1880: bla 0xb510   (init do IIC: grava 0xea00/0x2000000000000000)
       0x1884: bl  0x3c4     (zera dcache/linha, r4 blocos)
       0x188c: bl  0x2e8     (devolve o endereço 0x6a00000001f60000)
       0x1890: bl  0x3c4
       0x1898: ... (setup de MMU/SLB)
       0x1918: ... (grava SDR1/PCR em 0x130/0x131)
       0x1938: bl  0x2e8
       0x1944: mtspr 0x131, r3   (publica o PCR)
       0x1958: beql 0x364      (caminho de thread secundária)
       0x195c: slbia / laço de carga de SLB (0x1964..)
  caminho secundário 0x364: bl 0x218 (entra em modo HV / desabilita interrupções)
```

A entrada de hypercall (`0x77c`, tabela em `0x80`) despacha por `0xae80` para
handlers nas faixas `0x0xxx`/`0x2xxx` (offset de arquivo); as hypercalls de
segurança caem no par `0x84e0`/`0x87a8` e no setup de `0xa458`.

## 6. Constantes e padrões suspeitos

- `0x42440302` em `0x2034`: comparado com a palavra em `+0x20` de uma estrutura
  de dispositivo (provavelmente checagem de ID ou revisão).
- `0x71700` em `0x2c1c`: comparado com um contador em `0x6aa0(r10)` (gate de
  versão ou revisão).
- `0xc0` (base do vetor do IIC) gravado em `0x1910`.
- `0x1010`/`0x1014`: offsets de registrador do IIC.
- global `0x6360(r2)`: área de save de 5 palavras para os registradores do
  security engine.
- Não há imediato `0x1b`/`0x63`/`0x9e3779b9` nem S-box de AES na imagem; a
  criptografia é delegada ao hardware do security engine por `0x84e0`.
