# Texto

## Idioma

Português do Brasil, em todos os arquivos. Não há exceção por tipo de nota, por
pasta ou por assunto.

Ficam no original, sem tradução e sem itálico:

- identificadores de código, nomes de tipo, funções e variáveis;
- nomes de registrador, instrução, SPR, endereço e campo de hardware;
- nomes de arquivo, diretório, binário e comando;
- nomes de estágio da boot chain (`CB_A`, `CB_B`, `CD`, `CE`, `CF`, `CG`);
- siglas (`NAND`, `eMMC`, `SMC`, `MMU`, `HV`, `LLE`, `HLE`, `XEX`, `CRC`);
- citação literal de fonte externa (título de patente, trecho de datasheet,
  mensagem de ferramenta).

Um arquivo tem um idioma só. Nota técnica não começa em inglês e termina em
português, nem o contrário.

### Termos fixos

| Escreva | Não escreva |
|---|---|
| `fuses`, eFuses | fusos |
| `thread` | linha de execução |
| `boot`, boot chain | inicialização (quando é o boot chain) |
| `dump` | extração |
| `flush` | descarga |
| `handler` | manipulador |
| `patch` | remendo |
| `homebrew` | programa caseiro |

## Voz

- Afirmativa e direta. "O CD termina em `0x100`", não "o CD é terminado em".
- O sujeito é a coisa, não a pessoa: "o teste compara", não "nós comparamos".
- Sem hedge: "não sei", "não foi testado", "é modelo" são afirmações completas;
  "provavelmente talvez" não é.
- Sem ênfase retórica: nada de "isso é crucial", "vale destacar", "é importante
  notar". Se é importante, mostre o dado.
- Sem pergunta retórica no meio do texto.

## Título

- Nomeia o assunto: `# Security engine`, `# Sincronização multicore`.
- Sem data no título. Sem `(2026-10-25)`, `(em andamento)`, `(novo)`.
- Sem `Implementation Plan`, `Overview`, `Notes`, `Guia completo`.
- Sem emoji, sem `!!`, sem caixa alta.

## Estrutura

- Uma ideia por seção.
- `##` para assunto; `###` só dentro de um `##`. Não pule nível.
- O primeiro parágrafo diz o que a seção contém, não o que ela vai conter.
- Não escreva seção sobre o próprio documento ("Convenções deste documento",
  "Como ler este guia", "Escopo deste texto"). O leitor chega pelo título.
- Link para outro documento em vez de repetir o conteúdo dele. Se a mesma coisa
  aparece em dois arquivos, um dos dois está sobrando.

## Formatação

- **Tabela** só para dado tabular: endereço, flag, arquivo, versão, comparação
  de termos. Prosa não vira tabela.
- **Negrito** para aviso de risco real ou nome de campo em destaque. Não use
  para ênfase de frase. Nunca duas palavras em negrito na mesma linha.
- **Bloco de código** com linguagem declarada (`powershell`, `text`, `rust`,
  `python`). Comando sempre completo e colável.
- **Code span** para identificador, endereço, arquivo e valor. Ao citar arquivo
  por nome, confira que ele existe.
- **Travessão** para aposto curto. Prefira vírgula, dois-pontos ou parênteses
  quando a frase já tem outro travessão.
- **Emoji**: nenhum, em nenhum arquivo. Estado de marco é texto: `feito`,
  `parcial`, `aberto`.
- **Riscado** (`~~texto~~`): nunca. Ver [edicao.md](edicao.md).

## Números

- Todo número que se pode reproduzir vem com o comando que o produz.
- Endereço com o mesmo formato do resto do arquivo (`0x80000102_000100F0`).
- Não escreva contagem de teste ("127 testes"). A contagem muda e o texto
  apodrece; quem quer saber roda o comando ou olha o CI.
- Não escreva data de execução. O commit já tem data.

## Arquivo

- UTF-8 sem BOM. Fim de linha LF.
- Uma quebra de linha no fim do arquivo.
- Linha de até 80 colunas, salvo link ou tabela que não quebra.
- Nome de arquivo em minúsculas, sem acento, com hífen.
