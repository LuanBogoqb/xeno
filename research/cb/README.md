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
