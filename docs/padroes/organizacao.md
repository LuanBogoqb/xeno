# Organização

Onde cada coisa mora, quando uma coisa nova pode existir, e o que não entra.

## O que existe

| Caminho | Conteúdo | No git |
|---|---|---|
| `crates/xenon-cpu/src/` | emulador: `cpu/`, `mem/`, `dev/`, e os módulos de topo | sim |
| `crates/xenon-cpu/src/bin/` | executáveis de linha de comando (`runcb`, `opscan`) | sim |
| `crates/xenon-cpu/tests/` | integração com os dumps, marcada `#[ignore]` | sim |
| `tools/` | análise em Python para o PC | sim |
| `tools/xbolib/` | base compartilhada dos scripts | sim |
| `tools/capture/` | o que compila no XDK e roda no console | fonte sim, build não |
| `tools/build/xenon-bltool/` | headers de compatibilidade de referência | sim |
| `tools/data/` | tabelas de terceiros (ordinal → nome) | sim |
| `research/<alvo>/` | nota de engenharia reversa do alvo | sim |
| `research/<alvo>/analysis/` | saída de ferramenta (disassembly, tabelas) | sim |
| `docs/` | guia por assunto | sim |
| `docs/plans/` | plano concluído, mantido como registro | sim |
| `docs/padroes/` | estas regras | sim |
| `assets/brand/` | identidade visual | sim |
| `.github/workflows/` | CI: build, clippy, testes de unidade, checagem das ferramentas | sim |
| `dumps/` | entrada e saída local: flash, estágios, capturas, chaves | não |
| `samples/` | binário analisado | não |
| `scratch/` | experimento descartável | não |
| `target/` | build | não |

`.remember/` e `.superpowers/` são estado local de ferramenta. Ficam fora do git
e fora do projeto: ninguém depende deles, e apagar não quebra nada.

## Onde cada coisa mora

| O que é | Onde |
|---|---|
| código que roda no emulador | `crates/xenon-cpu/src/` |
| executável que o usuário chama | `crates/xenon-cpu/src/bin/` |
| teste de unidade | `#[cfg(test)]` no próprio módulo, ou `src/<mod>/tests.rs` |
| teste que precisa dos dumps | `crates/xenon-cpu/tests/` com `#[ignore]` |
| script de análise no PC | `tools/` |
| código que roda no console | `tools/capture/` |
| saída de ferramenta que vale versionar | `research/<alvo>/analysis/` |
| nota de engenharia reversa | `research/<alvo>/README.md` |
| guia de uso, arquitetura, status | `docs/` |
| decisão de escopo | `docs/decisions.md` |
| dado do console | `dumps/`, fora do git |
| experimento | `scratch/`, fora do git |

## Critérios para criar

Antes de criar arquivo, responda as cinco. Uma resposta "não" cancela a criação.

1. Responde pergunta que nenhum arquivo responde hoje?
2. Tem lugar definido na tabela acima?
3. Entra no índice correspondente no mesmo commit (`docs/README.md`,
   `research/README.md`, `docs/tools.md` ou `README.md`)?
4. Não repete conteúdo que já existe em outro arquivo?
5. Se envolve dado do console, fica fora do git?

Regras por tipo:

- **Módulo Rust**: quando o arquivo passa de ~800 linhas ou quando o assunto
  pede teste próprio. Módulo novo entra com `mod` declarado e teste.
- **Crate nova**: só quando precisa compilar separado — dependência que não deve
  entrar no binário do emulador, ou ferramenta que não é do emulador. Crate nova
  é decisão registrada em `docs/decisions.md`.
- **Script**: entra em `tools/`, usa `xbolib` em vez de copiar função, e é
  listado em `docs/tools.md` no mesmo commit.
- **Nota de pesquisa**: um `README.md` por alvo. Acima de ~350 linhas, quebra em
  arquivo próprio dentro do alvo e linka a partir do README.
- **Guia**: entra em `docs/` e ganha linha na tabela de `docs/README.md`.
- **Diretório de primeiro nível**: custo alto, só com decisão registrada.
- **Diretório com mais de um assunto dentro**: precisa de índice — `README.md`
  próprio ou seção num guia de `docs/`. `tools/` é coberto por
  `docs/tools.md`; `crates/` por `docs/architecture.md`.

## Critérios para não entrar

- Binário de terceiro. Exceção: header ou tabela pequena, com licença
  compatível e crédito.
- Código copiado de outro projeto. Derive e cite; não cole.
- Dado do console, perfil, chave, captura.
- Arquivo gerado: `target/`, `.xex`, `.obj`, `.pdb`, `.map`, `.xdb`.
- Experimento que ainda não virou resultado. Fica em `scratch/`.
- Segunda implementação da mesma coisa. Estenda a que existe.
- Arquivo que só existe para guardar histórico. O git faz isso.

## Nomes

- Diretório e arquivo em minúsculas, com hífen, sem acento, sem espaço.
- Nome diz o assunto, não a etapa: `security.rs`, não `new_security.rs`.
- Proibido no nome: `utils`, `helpers`, `common`, `misc`, `temp`, `tmp`,
  `novo`, `old`, `v2`, `final`, `copy`, `backup`, `(1)`.
- Estágio de boot usa o nome oficial: `CB_A`, `CB_B`, `CD`, `CE`, `CF`, `CG`.
- Plano concluído leva a data no nome do arquivo (`2026-10-24-m1-...`). É o
  único lugar onde data no nome é permitida.

## Higiene

- `.gitignore` é atualizado no mesmo commit em que a pasta local nasce.
- Um commit, um assunto. Mensagem diz o que mudou, no imperativo.
- O commit compila e os testes de unidade passam.
- Não commitar trabalho pela metade que não compila.
- Log de depuração fica atrás de flag. Sem `dbg!` esquecido.
- Arquivo morto se apaga, não se comenta.
- Ao mover ou renomear, corrija todos os links e referências no mesmo commit.
- Comando que aparece em dois lugares (CI e guia) tem uma fonte só: o guia
  aponta para o arquivo de CI, ou o contrário.
- Antes de criar, procure: `git ls-files` e `Select-String` custam menos que um
  arquivo duplicado.

## Tamanho

- Arquivo de código acima de ~800 linhas, ou ~1.000 se for arquivo de teste:
  dividir por assunto, com teste em cada parte.
- Documento acima de ~350 linhas: dividir, e o README do diretório vira índice.
- Função que precisa de comentário para marcar onde começa e onde termina já
  está grande: dividir. O arm de opcode em `cpu/mod.rs` é a exceção conhecida,
  porque cada braço é curto e o tamanho vem da quantidade de instruções.
