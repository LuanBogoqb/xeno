# Texto produzido com assistência de programa

Regras duras. Não são recomendações de estilo: são condições para o texto entrar
no repositório.

Vale para qualquer texto em que um programa tenha participado da redação, da
revisão ou da reescrita, independentemente de quanto a pessoa mexeu depois.

> Princípio único: **o leitor não tem nada a ver com como o texto foi feito.**
> Se uma frase fala do processo, do executor, da sessão ou da máquina onde o
> texto foi escrito, a frase sai.

## 1. Não se nomeia o processo

Proibido, em qualquer arquivo fora de `docs/padroes/`:

- `agente`, `sub-agente`, `subagente`, `agent`, `agentic`, `worker`;
- `assistente`, `IA`, `AI`, `modelo`, `LLM`, `inteligência artificial`;
- `prompt`, `skill`, `tool`, `tool call`, `function call`, `plan mode`;
- `sessão`, `contexto da sessão`, `memória`, `memória do projeto`;
- nome de produto ou de ferramenta de geração (`Claude`, `GPT`, `Codex`,
  `Copilot`, `Gemini`, `image_gen`, `superpowers`).

A lista é fechada. Plural, abreviação, tradução, caixa diferente ou nome
comercial equivalente são a mesma violação.

Delegação também é proibida, mesmo sem a palavra: "análise delegada", "resultado
do executor automático", "disassembly feito pelo assistente".

## 2. Não se narra o processo

Proibido descrever como o trabalho foi conduzido:

- "verificado por SHA-256 em `C:\Users\...`";
- "antes desta etapa, os 22 arquivos foram copiados";
- "nenhuma alteração anterior foi descartada";
- "com autorização do usuário";
- "prompts usados";
- "tarefa concluída", "próximo passo do plano".

Se a informação é útil ao projeto, ela é sobre o **objeto**: o arquivo, o dump, o
endereço, a medição. Nunca sobre a **execução**: quem copiou, quantos arquivos,
em que pasta temporária.

## 3. Não se registra a máquina

Proibido em texto versionado:

- caminho de máquina local (`C:\Users\...`, `AppData`, `/tmp`, `Temp\opencode`);
- nome de diretório de trabalho ou de sandbox;
- caminho de arquivo temporário, mesmo como exemplo;
- nome de usuário do sistema.

Caminho em texto versionado é sempre relativo ao repositório (`dumps/stages/`,
`research/hv/`).

## 4. Não se documenta o próprio documento

Proibido:

- seção sobre o próprio texto: "Convenções deste documento", "Como este guia foi
  escrito", "Sobre a revisão", "Self-Review", "Notas de revisão";
- frase sobre o que o texto não cobre, no lugar do que ele cobre;
- "este README é apenas um índice", "este documento é um resumo";
- instrução de leitura: "comece por aqui", "leia na ordem";
- comentário sobre o estado do arquivo: "em construção", "recém-reescrito".

## 5. Não se assina

Proibido:

- trailer `Co-Authored-By:` com nome de modelo, em qualquer commit;
- `Generated with ...`, `Made with ...`, emoji ou marca de ferramenta;
- "escrito por", "revisado por", "gerado por" em qualquer arquivo;
- nota de rodapé de autoria.

A assinatura do repositório é `twinko` no fim da mensagem de commit, e só isso.

## 6. Não se deixa diário

Proibido:

- título ou seção com data (`## Resultado (2026-10-25)`);
- "Corrigido em", "Atualizado em", "Isso corrige o §2", "Correção:";
- parágrafo contando o que o texto dizia antes;
- riscado (`~~...~~`) para marcar o que caiu;
- lista de hipóteses descartadas mantida como seção do estado atual. Hipótese
  descartada vira uma linha no texto que a substituiu, ou sai.

## 7. Não se deixa instrução para executor automático

Proibido qualquer trecho que mande um programa fazer algo:

- "**For agentic workers:**", "REQUIRED SUB-SKILL", "use checkbox syntax for
  tracking", "execute as tarefas em ordem";
- checklist de ferramenta (`- [ ]` de plano de execução) como conteúdo do
  documento;
- preferência pessoal apresentada como regra do projeto ("commits terminam com
  X porque o dono pediu");
- qualquer coisa que só faça sentido para quem está executando a tarefa, e não
  para quem lê o resultado.

## 8. Como escrever no lugar

| Errado | Certo |
|---|---|
| "O sub-agente fez o disassembly do HV." | "O disassembly do HV está em `analysis/`." |
| "Prompts usados: `transparent_background=true`" | "Fundo transparente preservado nas variantes." |
| "For agentic workers: REQUIRE... " | (nada; o plano começa pelo objetivo) |
| "M2 encerrada (2026-10-26)." | "M2 fecha pelo lado do emulador." |
| "Corrigido em 2026-10-25: as chaves são por boot." | "As chaves são por boot." |
| "22 arquivos copiados para `Temp\opencode\...`" | (nada; o git guarda o estado) |
| "Este documento não cobre o Live." | "O Live depende do XOSC (ver `status.md`)." |

## 9. Onde isto é verificado

[checklist.md](checklist.md) traz os comandos de varredura. A varredura ignora
`docs/padroes/`, porque é esta pasta que precisa nomear o que proíbe.

Se um termo proibido aparecer fora daqui, não se negocia caso a caso: reescreve.
