# Padrões do repositório

Como se escreve e como o repositório se organiza. Vale para tudo que entra no
git: `docs/`, `research/`, `crates/`, `tools/`, READMEs, comentários de código,
mensagens de commit e texto de pull request. Vale para texto escrito por pessoa e
para texto escrito com assistência de programa. Não existe marcação de autoria:
o texto é do projeto.

## As regras que não se negociam

1. **Não se nomeia o processo.** Texto do repositório não menciona agente,
   sub-agente, assistente, modelo, prompt, sessão, ferramenta de geração nem
   nome de produto. O que importa é o método, nunca quem executou.
2. **Substitui-se, não se anexa.** Um fato ocupa uma frase no arquivo. Quando o
   fato muda, a frase é reescrita. O histórico fica no git, não no texto.
3. **Escreve-se em português.** Identificador, código, registrador, nome de
   arquivo e citação literal ficam no original; o resto é português do Brasil.
4. **Nada do console entra no git.** Dump, chave, perfil e captura ficam em
   `dumps/`, e chave em texto aparece mascarada.
5. **Criar tem critério.** Arquivo novo responde pergunta que ninguém responde,
   tem lugar na estrutura e entra no índice no mesmo commit. Sem índice, é
   arquivo morto.

## Arquivos

| Arquivo | Responde |
|---|---|
| [texto.md](texto.md) | idioma, voz, estrutura, formatação, números, links, código |
| [ia.md](ia.md) | o que nunca aparece em texto produzido com assistência de programa |
| [edicao.md](edicao.md) | como alterar um texto sem deixar camadas de versões antigas |
| [organizacao.md](organizacao.md) | onde cada coisa mora, quando criar, quando não criar, nomes, higiene |
| [seguranca.md](seguranca.md) | segredos, dados do console, rede, terceiros, dependências |
| [checklist.md](checklist.md) | verificação mecânica antes do commit |

## Onde se fala de IA

Aqui, e só aqui. Esta pasta é a única parte do repositório autorizada a escrever
as palavras listadas em [ia.md](ia.md). Se um texto fora desta pasta precisar
mencionar o assunto, o texto está errado e deve ser reescrito.

A verificação de [checklist.md](checklist.md) ignora `docs/padroes/` por esse
motivo: os padrões precisam nomear o que proíbem.
