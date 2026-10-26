# CB_A → CB_B → CD: execução até o handoff

## Resultado (2026-10-25)

O CD termina no emulador: **271.049.701 instruções**, incluindo o CF chamado pelo CD.
O pequeno acréscimo em relação ao valor original (271.049.695) vem da resposta
real do SMC `0x01` capturada do console, que altera um ramo do boot.
Decifra/descomprime CE, autentica e executa CF, aplica CG e os 57 descritores de
patch da NAND, transporta os argumentos de boot e executa o `rfid` final.
Estado de saída: **PC `0x100`, MSR `0x1000000000000000`, HRMOR `0x10000000000`**.
O runner para antes de buscar a primeira instrução do HV/kernel; essa execução
posterior não faz parte do resultado validado aqui.

```powershell
cargo run --release -p xenon-cpu --bin runcb -- dumps/stages/CB_A.mem dumps/FLASH_1.bin --stage cd --max-steps 400000000
cargo test -p xenon-cpu --release
cargo test -p xenon-cpu --release --test chain -- --ignored
```

## Causa do bloqueio anterior

- Não faltava descartar uma escrita em `0x15d9d8`: o CF tinha sido rejeitado,
  deixando apenas o kernel 1888 (`0x120000` bytes), incompatível com os patches
  finais destinados ao kernel 17559 (`0x1F0000` bytes).
- A chave RSA recebida pelo CD estava zerada: faltava o bloco da 1BL apontado por
  SRAM+`0xFC`. `map.rs` inicializa a chave pública do boot-ROM e a chave HMAC
  conhecida; o próprio CD faz RSA/HMAC e retorna sucesso, sem override.
- A posição `0x200` desse bloco é uma escolha do modelo em SRAM livre, não um
  endereço de ROM confirmado. Fonte dos valores públicos: xenon-bltool
  `source/1bl-keys.c` (dados numéricos, sem copiar a implementação).
- Implementados `mulhdu` e `mtcrf`/`mtocrf`, exercitados pelos caminhos RSA/CF.
- Removidas as hipóteses antigas que zeravam FUSE traduzido e devolviam `0x48`
  em EA `0x14`. Agora os fuses originais autenticam o CF e produzem LDV **7**;
  B7/B9 passam com o conteúdo real. Também removido zero/descarte genérico
  para acessos traduzidos cujo alvo físico não existe.

## Limites do modelo

O TLB continua experimental: formas observadas `2/4/8`, sem SLB/VSID/proteções
completas. IR e DR são independentes; um miss real continua sendo erro.
O CD muda a página de staging `0x3C0000` para RPN `0x178003C0193` antes de
chamar CF com IR desligado. Modelamos esse caso como janela de instruções de
cache travado anterior ao HRMOR, usando o mesmo backing da página; a troca
de volta para `...196` remove a janela. Não é fallback genérico de tradução
nem uma decodificação completa do cache Xenon. Há testes de isolamento.

Permanecem os atalhos anteriores de escopo RGH: compare de digest do CB_A,
reposição do CB_B plaintext e salto do script de configuração de hardware
do CB_B. **Nenhum override de verificação no CD/CF e nenhuma alteração dos fuses.**
SMC e vários registradores SoC ainda são modelos mínimos, não hardware completo.

## Regressão com os dumps locais

Os quatro testes ignorados verificam CD em RAM, kernel-base byte-idêntico,
kernel 17559 byte-idêntico imediatamente após o CF e imagem final completa
(flags/LDV, argumentos de boot e patches da NAND) no handoff. Os arquivos offline
existentes são apenas oráculos dos testes: não são injetados no runner.
`--cd-stop-pc HEX` permite snapshots em fronteiras internas para esses testes.
