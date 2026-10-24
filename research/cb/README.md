# CB_A (estágio 2BL) — varredura estática

Conferido em 2026-10-24 com `python tools/cbscan.py dumps/stages/CB_A.bin`, Python 3.14 e Capstone 5.0.7. Imagem decifrada: `0x1AF0` bytes, entry `0x3D0` (offset no arquivo); 1.480 palavras varridas em PPC64 big-endian. A ferramenta lê o entry do header em `+0x8` e continua após palavras não decodificadas.

Para reproduzir sem instalar dependências no Python global (PowerShell, na raiz do projeto):

```powershell
python -m pip install --target .superpowers/cbscan-deps capstone==5.0.7
$env:PYLIB = (Resolve-Path .superpowers/cbscan-deps).Path
python tools/cbscan.py dumps/stages/CB_A.bin
```

O dump e a dependência local ficam em diretórios ignorados pelo git.

## Mapa de memória (da sequência de entrada em `0x3D0`)

| Endereço | Papel | Evidência |
|---|---|---|
| `0x8000_0200_0001_0000` | **Base de carga do CB_A** (início do header, SRAM) | `r2` recebe esse valor, `lwz r3,0xC(r2)` lê o tamanho do header |
| `0x8000_0200_0001_C000` | Cópia relocada do CB (`+0xC000`) | laço `ldu/stdu` copia `(size+0xF)/8` doublewords e `lr += 0xC000` |
| `0x8000_0200_0001_F300` | Stack (`r1`) | `0x3EC..0x3FC` |
| `0x8000_0200_0006_0000` | Bloco de registradores do SoC | `std r4,0x1010(r3)` |
| `0x8000_0200_0006_1010` | Registrador que recebe `0xD000_0000_0000_0000` na entrada e `0xF300_0000_0000_0000` na falha | `0x3E8` e `0x498` |

Em `0x41C`, se o tamanho do header for `>= 0x2000` o código vai para `0x480` (escreve o marcador de falha e para com `mtspr 0x98, r0` em laço). Nosso CB_A tem `0x1AF0`, então segue.

Depois da cópia, `r3 = r31`, `r4 = base original`, `r2 += 0xC000` e salta para `0x600` (relativo; o código é independente de posição).

### Evidência e ambiguidade da base (item c)

A sequência `0x400..0x40C` constrói `r2 = 0x8000_0200_0001_0000`; em `0x414`, `lwz r3,0xC(r2)` lê o tamanho do header nessa base. Esse é o endereço de carga provável adotado para a Task 5. A cópia para `base + 0xC000` e o ajuste de LR em `0x460..0x468` sustentam a interpretação de relocação.

A conferência dos campos de opcode, deslocamento e AA das palavras encontrou **199 branches diretos**, todos relativos, com destinos dentro de `[0, 0x1AF0)`. Portanto, o teste de conter os saltos **não escolhe uma base**: qualquer translação que não cause overflow preserva os destinos relativos. O menor valor por esse critério isolado seria zero, mas não explicaria a leitura absoluta do header. Adotamos a base construída no código; não há prova de unicidade por branches. Saltos indiretos por LR/CTR exigem análise do fluxo ou execução.

## Estado que a 1BL deixa (item d)

- `r0`: lido em `0x410`, `ori r6,r0,0xC000`, antes de qualquer escrita. Usar zero produz o deslocamento esperado de relocação (`0xC000`); é uma hipótese de estado inicial, não uma medição da 1BL.
- `r31` = argumento da 1BL, repassado ao código em `0x600`. Valor desconhecido; usar 0 e revisar quando o código o desreferenciar.
- A heurística das primeiras 64 palavras lineares encontra apenas `r0` e `r31`. Isso não prova o estado de todos os caminhos: não segue branches/chamadas, não modela SPRs e não substitui a validação da 1BL.

## Instruções usadas (item a)

Mais frequentes: `addi li add mr bl std ld cmplwi lwz stw rotlwi xor mtctr ori beq bdnz b clrlwi subf lis blr bne sldi lbz and srwi oris stdu clrldi or bge stb mflr`. Cauda: `rlwimi cmpld cmplw lbzu blt lbzx subfic rlwinm stbu andi. cmpwi nor stbx extsw lwzx mtlr mtspr rldimi bgt lhz rldicr lwzu stwu addis slwi sth ble bdnzf bdz blrl rldicl cmpldi stwx`, mais as formas `beqlr/bltlr/bdzlr`. 41 palavras não decodificam (`.long`); podem ser dados e serão conferidas na Task 3.

## Constantes (item b)

- Sem constantes de SoC além das da tabela acima.
- O CB_A é **criptografia em software**: constantes SHA-1 (`67452301 EFCDAB89 98BADCFE 10325476 C3D2E1F0`, `5A827999 6ED9EBA1 ...`), pads de HMAC (`0x36`/`0x5C`) e uma tabela de RC4. Não há leitura de MMIO de flash neste trecho de código; quem lê o CB_B é o código em `0x600` em diante.

Offsets da primeira formação e usos identificados na inspeção:

| Constante | Offset | Uso |
|---|---|---|
| `0x8000020000060000` | `0x3DC` | Base do SoC; store em `+0x1010` em `0x3E8` |
| `0xD000000000000000` | `0x3E4` | Marcador inicial escrito no SoC |
| `0x800002000001F300` | `0x3FC` | Stack em `r1` |
| `0x8000020000010000` | `0x40C` | Header em `r2`, tamanho lido em `+0xC` |
| `0x800002000001BFF8` | `0x430` | Cursor de destino antes do primeiro `stdu`, que soma 8 |
| `0xF300000000000000` | `0x484` | Marcador do caminho de falha |
| `0x67452301`, `0xEFCDAB89`, `0x98BADCFE`, `0x10325476`, `0xC3D2E1F0` | `0xB80..0xB90` | Estado inicial SHA-1; alguns valores têm extensão de sinal no GPR64 |
| `0x5A827999`, `0x6ED9EBA1` | `0xC74`, `0xD60` | Constantes de rodadas SHA-1 |
| `0x3636363636363636`, `0x5C5C5C5C5C5C5C5C` | `0x1324`, `0x1350` | Pads interno/externo de HMAC |

O rastreador de constantes é linear e ignora fluxo de controle. Sua coluna `first at` é a primeira formação numérica, não uma prova de uso: a base `0x8000020000010000` já aparece transitoriamente em `r1` em `0x3F8`, antes de ser construída em `r2`. Valores intermediários e padrões sem uso demonstrado devem permanecer candidatos, sem interpretação de hardware.

## Outros (itens c, e)

- Nenhum branch absoluto (`ba/bla`).
- Único SPR tocado: `0x98` (CTRL), só como halt de falha. Não há `sc`, `rfid`, `mtmsr` nem instruções de cache.

## Primeira execução (`runcb`, Task 5)

`cargo run -p xenon-cpu --bin runcb -- dumps/stages/CB_A.bin`: 6 instruções e para no primeiro acesso ao SoC.

```
steps: 6
stop: Unmapped { addr: 0x8000_0200_0006_1010, size: 8, write: true }   # std r4,0x1010(r3), r3=0x8000020000060000, r4=0xD000...
```

Confere com o mapa acima. Próximo passo: mapear o bloco do SoC como dispositivo (Task 6).

## Histograma completo (item a)

Varredura linear, sem separar código de dados. `.long` conta palavras não decodificadas; aliases e sufixos de predição do Capstone são preservados.

```text
   176 addi
   126 li
   104 add
    99 mr
    73 bl
    65 std
    55 ld
    50 cmplwi
    47 lwz
    41 stw
    41 .long
    41 rotlwi
    37 xor
    32 mtctr
    28 ori
    25 beq
    24 bdnz
    23 b
    22 clrlwi
    22 subf
    20 lis
    17 blr
    17 bne
    15 sldi
    15 lbz
    15 and
    14 srwi
    13 oris
    12 nop
    12 stdu
    11 clrldi
    11 or
    10 bge
    10 stb
     9 mflr
     8 rlwimi
     7 cmpld
     7 cmplw
     7 lbzu
     7 bdnz+
     6 blt
     6 lbzx
     6 subfic
     5 rlwinm
     5 stbu
     5 andi.
     4 ldu
     4 cmpwi
     4 nor
     4 stbx
     4 extsw
     4 lwzx
     3 mtlr
     3 rldimi
     3 bgt
     3 lhz
     3 rldicr
     3 beqlr-
     3 lwzu
     3 stwu
     3 rlwinm.
     2 mtspr
     2 addis
     2 slwi
     2 sth
     2 ble
     2 bdnzf
     2 beq-
     2 beqlr+
     2 bdz-
     2 bdzlr-
     1 blrl
     1 rldicl
     1 bltlr
     1 bne+
     1 cmpldi
     1 stwx
     1 bne-
     1 beq+
```

## Task 6 — estado (parcial)

`cargo run -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin [addr:len ...]`
(`CB_A.mem` = CB_A decifrado com a chave derivada gravada em `+0x10`, como a 1BL deixa; gerado com `HMAC-SHA1(1BL key, salt)[:16]`).

Descoberto:
- `r31` (arg da 1BL) = **offset de flash do próximo estágio** (CB_B, `0x9EF0`); o código em `0x840` exige alinhado a 16 e em `0x80..0x7FFFF80`. A flash aparece em `0x8000_0200_C800_0000` (janela de 128 MiB).
- CB_A relê o cabeçalho do próximo estágio pela janela, copia o corpo para a base da SRAM, deriva `key = HMAC(CB_A.key, salt || 16 zeros)[:16]` (função `0x13d8`; confere com Python), decifra `[0x20, size)` com RC4 (confere com Python) e compara um digest (função `0x950`, 20 bytes) com o armazenado em `CB_A+0x39C`.
- Instruções que faltavam: só `stbu`. O interpretador roda ~14k passos até o header e o laço SHA/RC4 completo sem erro.

Resolvido: o digest é RotSumSha (função `0x950`): acumula 4×u64 com soma/rotação sobre palavras de 8 bytes de `hdr[0:0x10]` e `corpo[0x140:size]`, depois SHA-1 de `estado,estado,buf1,buf2,~estado,~estado`. Reimplementado em Python e bate bit a bit com o interpretador (`8f8b9a07…` para o CB_B). O esperado em `CB_A+0x39C` (`717bccb4…`) **não** bate com o CB_B deste dump: é o CB_B regravado do RGH3, e o console real passa por glitch nessa comparação. Pelo escopo "sem glitch", `runcb` força o resultado da comparação em `0x7F0` (relocado) para "igual" e registra `hash_ok=false`.

Resultado: `HANDOFF pc=0x20003d0 hash_ok=false` após ~2,03 M instruções. A rotina `0x4A8` zera os GPRs, passa `r31 = 0x11A30` (offset do CD, próximo estágio) e salta para `0x0200_0000 + (entry & 0xFFFF)`. Esse endereço de fetch ainda não está mapeado (a SRAM aparece em `0x8000_0200_…`); o alias real-mode fica para o plano do CB_B.

Critério de saída atingido (PC no entry do CB_B), com o hash forçado.

## Alvo real do handoff (M1b, Task 1)

Desmontagem do CB_A em `0x4A8..0x4B0`: `clrlwi r3,r3,16; addis r3,r3,0x200; mtlr r3`. `clrlwi` zera os 32 bits altos, então o alvo é o endereço de **32 bits** `0x0200_0000 + (entry & 0xFFFF)`, não `0x8000_0200_…`. Regra adotada: alias no `Bus` (`map_alias(0x0200_0000, 0x1_0000, CB_A_LOAD)`), de modo que `0x0200_03D0` lê `0x8000_0200_0001_03D0`, onde o CB_A deixa o CB_B (header `CB3A` em `SRAM+0x10000`, medido no dump pós-handoff; a base `SRAM+0` fica zerada) (a rotina `0x4A8` zera os GPRs e `r31 = 0x11A30`). Não há prova de hardware dessa equivalência; é a que faz o corpo decifrado ficar executável.

## CB_B (M1b, Task 2) — varredura estática

Conferido em 2026-10-24 com `cargo run --release -p xenon-cpu --bin opscan -- dumps/stages/CB_B.bin > unk.txt` e `python tools/cbscan.py dumps/stages/CB_B.bin unk.txt` (a ferramenta ganhou a seção `(a2)`: mnemônicos que o interpretador não decodifica; `opscan` testa decodificando de fato, não por lista).

**CB_B.bin é texto claro no dump RGH3** (`tools/bootchain.py`: "RGH image stores CB_B already decrypted"; o campo `key` do header `+0x10` é a chave usada para derivar a do CD). Consequência: o `CB_B` que o CB_A decifra na SRAM (`dumps/stages/CB_B.mem`) é ruído; o runner precisa **sobrescrever** a SRAM `CB_A_LOAD..+0x7B40` com `CB_B.bin` no handoff (Task 4). Imagem: `0x7B40` bytes, entry `0x3D0`. Cabeçalho `CB3A`, flags `0x0801` no `.mem` contra `0x0800`-família no arquivo.

Código e dados se misturam: blocos `0x1000..0x4800` têm entropia de dado (histograma sem sentido: `xsaddsp`, `vmsum*`, `twi`, `addpcis`, 1.957 `.long`), código claro em `0x3D0..0x1000` e `0x5000..0x7800` (SHA/RC4/HMAC como no CB_A). Os mnemônicos ruído (FP, VMX, `twi`, `addpcis`, `attn`) **não** entram na Task 3.

### (a2) Mnemônicos de código que faltam no interpretador (candidatos; confirmar por execução)

`sync isync dcbst dcbzl mfmsr rfid mftb mulli mullw mulld divwu divdu addic addic. subfe xori xoris andc cntlzw slw srw sld srd rlwnm rlwnm. rldcr rotlw lha lhbrx sthbrx lwbrx stwbrx ldx stdx mcrf mfocrf` mais o word `0x7C201A24` na entrada (`0x3D4`, `lis r3,0x100` antes), não decodificado pelo capstone.

### (b) Endereços absolutos

| Endereço | Visto em | Papel provável |
|---|---|---|
| `0x8000_0200_0006_0000` | `0x3E8` | bloco SoC; `std 0x2000_..._0000 → +0x1010` na entrada (marcador de estágio, como `0xD000..` no CB_A) |
| `0x8000_0200_0001_0000` | `0x404`, `0x41C` | base da SRAM da imagem; `lhz/sth +6` (flags do header): `andc 0x600`, `ori 0x20` |
| `0x8000_0200_0001_F300` | `0x408` | stack (`r1`), igual ao CB_A; `0x440` zera `0x680` doublewords abaixo de `r1+0x100` se bit 0x800 estava ligado |
| `0x8000_0200_0003_0000` (+`0x20`) | `0x45C..0x46C` | dispositivo desconhecido; escreve `0x2000_0000_0000_0000` e relê (`isync` ao redor) |
| `0x8000_0200_0006_1000` (+`0x1A0`) | `0x490..0x498` | SoC; escreve `0x1FF` |
| `0x8000_0200_E104_0000` | `0x720` | desconhecido |
| `0x8000_017A_0300_xxxx`, `0x8000_017A_0301_xxxx` | ~60 constantes, `0x550..0x5570` | **bloco grande de registradores** (candidato: controlador de DRAM/PCI bridge; em Free60 o northbridge fica em `0x…EA00_xxxx`, não confere, então tratar como não identificado) |
| `0x8000_017C_0000`, `0x8000_017C_0400` | poucas | idem, bloco vizinho |

O rastreador de constantes é linear e cego a fluxo; endereços de dado do bloco `0x1000..0x4800` podem ser falsos.

### (c) SPRs, (d)

SPRs tocados em `0x49C..0x6D8`: `0x3F9`, `0x13E`, `0x3F4`, `0x3F1` (leitura-modificação-escrita com `sync/isync`, estilo HID/config de CPU), `0x3B3/0x3B4/0x3B5`, `0x139`, `0x1A/0x1B` (SRR0/SRR1: monta o retorno e **`rfid` em `0x6D8`**). Há um segundo bloco igual em `0x7D0..0x83C`. `mtspr 0x98` em `0x7B8` (CTRL, halt de falha). O interpretador devolve 0 para SPR nunca escrito, então os `mfspr` passam, mas o `rfid` precisa de implementação (Task 3) e o que o código espera de MSR depois dele define o modo de endereçamento (MMU ainda desligada?).

Digest: não localizado ainda; procurar pela soma de rotação nas funções SHA em `0x5000..0x7800` e pela comparação com 20 bytes do header do CD. O CD é lido pela flash em `0x11A30` (a janela `0x8000_0200_C800_0000` do CB_A é a candidata a repetir).

## Execução do CB_B (M1b, Task 4)

`cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage cb_b` termina em `HANDOFF pc=0x4000300 digest_overrides=1` após 2.172.139 passos no CB_B (o `1` é o override do CB_A; o CB_B não precisou de nenhum). Variáveis: `XB_TRACE_MMIO=1` loga todo acesso a registrador; `--log-pc PC:r,r` imprime GPRs a cada passagem por um PC.

O corpo do CD que o CB_B decifra na janela 2 da cache bate com `dumps/stages/CD.bin` byte a byte, exceto o header (bytes `0x7` e `0x10..0x1F`, reescritos pelo CB_B) e o byte `0x24A` (flag depois da string `XBOX_ROM_6`, 0 no arquivo e 1 depois do CB_B). Teste: `cargo test -p xenon-cpu -- --ignored` (`cd_in_ram_matches_stage_file`).

### O que o CB_B faz (evidência de execução)

1. Entrada `0x3D0`: marca POST no SoC (`+0x1010`), mexe em SPRs de configuração (`0x3F9`, `0x13E`, `0x3F4`).
2. Lê as **fuses** em `0x8000_0200_0002_0000 + 0x200*n` (12 linhas de 64 bits, `0x4F3C..0x4FA0`) e confere a linha 0 (`0xC0FF…`); sem elas aloca POST `0x9B`. Dispositivo `dev/fuses.rs`, alimentado por `dumps/FUSE.bin`.
3. **Cache travada como RAM**: `dcbzl` em `0x8000_017A_0300_0000` (64 KiB), copia a si mesmo para `+0x3000`, configura as SPRs `0x3B3..0x3B5` e `HRMOR` (`0x139`) e salta com `rfid` para `0x0300_36F0`. Depois zera uma segunda janela em `0x8000_017C_0400_0000`. As duas janelas respondem em EA `0x0300_0000` e `0x0400_0000` (alias no `Bus`); o `HRMOR` (OR em endereço com bit 63 zerado) também foi modelado, com efeito adiado até o próximo `isync`/`rfid` ou até a instrução seguinte.
4. **Interpretador de bytecode** de programação de registradores (`0x9A8..0xDD4`, programa em `0x1000..0x4C50` do arquivo): palavras de 32 bits, opcode nos 6 bits altos (`<8` salto condicional r5 vs r6, `8/9` escrita, `10` leitura, `11` call/return, `20..` aritmética), seletores de operando nos bits `20..16` e `15..11`. Escreve em PCI config (`0x8000_0000_D000_xxxx`, `E400_15xx`, `EA00_xxxx`), conversa com a SMC e testa o DRAM. **HLE**: o `runcb` pula o programa inteiro (`Action::Jump` de `0x9A8` para `0xDDC`, r5 = 0), porque ele só programa controlador de memória/PCI/SMC; o DRAM (512 MiB em `0x8000_0000_0000_0000`) fica sempre mapeado. A SMC e as checagens que o programa fazia antes do hook estão modeladas mesmo assim (abaixo), para o caso de o hook sair.
5. `lwbrx` do chip id em `0x8000_0200_E104_0000` (parte alta `>= 0x2000` ou POST `0xAF`; `>= 0x4000` liga o flag `0x80` do header).
6. Lê o CD da flash, decifra, **escreve o CD na janela 2**, faz `dcbst` nas duas janelas, reprograma `0x3B3..0x3B5` para `0x408/0x405` e salta com `blr` para `0x0400_0000 + (entry & 0xFFFF)`.

### Mailbox da SMC (BAR em `0xEA00_1000`, registradores little-endian)

Ordem tirada do bytecode (`0x2D1C..0x2DC0`): enviar = escrever `4` em `+0x84`, 4 palavras em `+0x80`, `0` em `+0x84`; receber = esperar bit 2 de `+0x94`, escrever `4` em `+0x94`, ler 4 palavras de `+0x90`, escrever `0` em `+0x94`. O laço de espera sai para o caminho de envio quando bit 2 de `+0x84` está ligado. O primeiro comando é `0x12` (consulta de versão) e a resposta deve repetir `0x12` no primeiro byte. A SMC já nasce com uma mensagem zerada pendente. **Palpite**: os bytes `41 02 03` da resposta de versão e o conteúdo da mensagem inicial; nada nos dumps registra isso.

### Palpites e valores sem prova (listados pra poder trocar)

- `E400_15EC`: bits 8..11 forçados em 4 (`(reg >> 8) & 0xF == 4`, estado do link).
- Chip id `0x5000_0000` LE: só precisa passar de `0x4000`.
- `0x6_1000 = 0x2000_0000_0000_0000`: bit 61 = "fuses com dados".
- Região "dram-hole" (`0x8000_0000_2000_0000..`) lê/escreve de volta; o programa sonda `0x2000_1800`.

## CD (M1b, Task 5) — estado parcial, parado na MMU

`runcb ... --stage cd` roda o CD por ~43.400 passos e para em `Unmapped read 4 bytes at 0x17a130000fc` (`CD 0xBA0`: `lis r11,0x1000; lwz r10,0xFC(r11)`). Antes disso o CD:

- marca POST no SoC, fala com o BAR em `0x8000_0200_EA00_1000` (alias do `0x8000_0000_EA00_1000`; `+0x14` recebe `0x2E00_0000` e `+0x18` bit 6 diz "pronto");
- usa `slbmte` e `mtmsrd` (agora implementados como estado, sem tradução) e liga `MSR.IR` (`msr=0x1000_0000_0000_0010`);
- monta uma **TLB por software** com `mtspr 0x3B3/0x3B5/0x3B4` (96 escritas, `XB_TRACE_SPR=1` as lista). Padrões vistos: (A) 8 entradas de 16 MiB, `3B4=0x18xx..0x1Fxx`, `3B5=0xC8xx_11BF`: EA `0x1800_0000..0x1FFF_FFFF` -> janela de flash; uma entrada EA `0x1000_0000` -> `0x0200_0000_xxxx` (`3B5=0x0200_0000_11BE`); (B) entradas de 64 KiB identidade nos EAs `0x28_0000..0x3D_0000` (`3B5` = EA<<17 | 1<<40 | EA | 0x196; função em `CD 0x490`).

**Bloqueio**: o significado exato de `3B3/3B4/3B5` (qual é índice, qual é VPN, tamanhos de página, quando o `3B4` grava a entrada) não está documentado em lugar nenhum que eu tenha achado (Free60 e buscas gerais não trazem). A leitura em `EA 0x1000_00FC` depende dela: precisa saber a que endereço real o EA `0x1000_0000` aponta. Daqui em diante (CD, CE, CF/CG) a execução depende de um modelo de TLB/MMU decidido com base em palpite.
