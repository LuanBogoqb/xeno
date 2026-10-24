# CB_A (estágio 2BL) — varredura estática

Gerado por `python tools/cbscan.py dumps/stages/CB_A.bin` (saída completa abaixo, resumida). Imagem decifrada: `0x1AF0` bytes, entry `0x3D0` (offset no arquivo).

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

## Estado que a 1BL deixa (item d)

- `r0 = 0` (lido em `ori r6,r0,0xC000` antes de qualquer escrita).
- `r31` = argumento da 1BL, repassado ao código em `0x600`. Valor desconhecido; usar 0 e revisar quando o código o desreferenciar.
- Todo o resto o código inicializa.

## Instruções usadas (item a)

Mais frequentes: `addi li add mr bl std ld cmplwi lwz stw rotlwi xor mtctr ori beq bdnz b clrlwi subf lis blr bne sldi lbz and srwi oris stdu clrldi or bge stb mflr`. Cauda: `rlwimi cmpld cmplw lbzu blt lbzx subfic rlwinm stbu andi. cmpwi nor stbx extsw lwzx mtlr mtspr rldimi bgt lhz rldicr lwzu stwu addis slwi sth ble bdnzf bdz blrl rldicl cmpldi stwx`, mais as formas `beqlr/bltlr/bdzlr`. 41 palavras não decodificam (`.long`); podem ser dados e serão conferidas na Task 3.

## Constantes (item b)

- Sem constantes de SoC além das da tabela acima.
- O CB_A é **criptografia em software**: constantes SHA-1 (`67452301 EFCDAB89 98BADCFE 10325476 C3D2E1F0`, `5A827999 6ED9EBA1 ...`), pads de HMAC (`0x36`/`0x5C`) e uma tabela de RC4. Não há leitura de MMIO de flash neste trecho de código; quem lê o CB_B é o código em `0x600` em diante.

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
