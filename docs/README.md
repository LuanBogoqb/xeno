# Documentação

Mapa dos guias. O [README](../README.md) é só o índice; cada documento abaixo
tem um assunto e linka as notas profundas em `research/`.

| Documento | Responde | Público |
|---|---|---|
| [architecture.md](architecture.md) | como o emulador é montado: módulos, fluxo de boot, mapa de memória, HV, threads | quem vai mexer no código |
| [status.md](status.md) | marcos M0–M7, o que está validado e o que falta | visão geral |
| [getting-started.md](getting-started.md) | compilar, rodar, testar, onde ficam os dumps | quem acabou de clonar |
| [capturing-from-console.md](capturing-from-console.md) | como extrair dados do console (plugin de kernel, launch.ini, FTP) | quem tem o hardware |
| [plugins.md](plugins.md) | como o emulador carrega plugin de kernel, do jeito do Dashlaunch | quem quer carregar Proto ou um plugin próprio |
| [security-engine.md](security-engine.md) | a cifra de memória: algoritmo, chaves por boot e validação em hardware | quem for mexer em `security.rs` |
| [tools.md](tools.md) | cada script/binário de `tools/` e para que serve | uso diário |
| [decisions.md](decisions.md) | decisões de escopo (LLE, sem glitch, Live, nada da MS no repo) | antes de propor mudanças grandes |
| [padroes/](padroes/) | texto, organização e segurança: como escrever, onde cada coisa mora, o que não entra no git | antes de criar qualquer arquivo |
| [plans/](plans/) | planos históricos de M1/M1b, mantidos como registro | contexto |

Notas profundas (engenharia reversa, disassembly, experimentos) ficam em
[`research/`](../research/README.md) e não são reescritas aqui: os guias acima
apontam para elas. Números reproduzíveis vêm com o comando que os produz, e o
que é modelo aparece dito como modelo.
