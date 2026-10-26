# Edição

O repositório tem um estado. O histórico do que já foi está no git, não no texto.

## Regra central: substituir

Quando um fato muda, **reescreva a frase que o afirma**. Não acrescente
parágrafo novo, não abra seção "atualização", não deixe o antigo com uma nota
dizendo que envelheceu.

Errado:

```
M2 não está concluído. (parágrafo de outubro)
...
M2 fechado no escopo atual. (parágrafo de novembro)
...
M2 encerrado. (parágrafo de dezembro)
```

Certo: uma frase, no presente, com o estado de hoje.

## Um fato, um lugar

- Se a mesma informação aparece em três arquivos, dois estão sobrando. O fato
  mora em um lugar; os outros apontam para ele com link.
- Ao corrigir um valor (default de flag, contagem, endereço), procure todas as
  ocorrências antes de editar. Corrigir uma e deixar duas é pior do que não
  corrigir nenhuma, porque o leitor passa a não confiar em nenhuma.
- Estado de marco mora em um lugar só: a tabela de `docs/status.md`.

## Proibido na edição

- `Corrigido em ...`, `Correção:`, `Atualizado em ...`, `Obs.:`, `Nota:`,
  `Update:`, `Novo!`;
- riscado (`~~texto~~`) para marcar o que saiu;
- "este trecho substitui o anterior", "ver versão antiga";
- seção `Histórico`, `Changelog` ou `Mudanças` dentro de documento de assunto;
- data em título;
- comentário sobre a revisão ("revisado para refletir X").

## Quando duas partes discordam

1. Descubra qual está certa olhando o código, o dump ou o teste.
2. Apague a errada. Não a mantenha com uma ressalva.
3. Se a divergência for histórica e valiosa (uma hipótese que caiu e que alguém
   poderia repetir), ela fica em uma linha do texto que a substituiu, no tempo
   presente e sem narrar a investigação.

## Antes de fechar a edição

- Procure contradição entre arquivos: mesmo default, mesma contagem, mesmo
  endereço citado em mais de um lugar.
- Procure referência a arquivo que não existe. Link em Markdown é verificado
  pelo GitHub; nome em `code span` não é, e é onde o erro costuma ficar.
- Procure o mesmo assunto tratado em duas seções do mesmo arquivo.
- Leia o diff como se fosse de outra pessoa: se alguma linha conta o que o texto
  dizia antes, ela está errada.
