# Segurança

Regras curtas, todas verificáveis, sobre o que pode sair deste repositório.

## Segredos

Nunca entram no git:

- CPU key, keyvault, fuses;
- chaves de DRAM do security engine (`hash`, `white`, `aes`);
- dump de NAND ou eMMC, estágio decifrado, captura de RAM;
- perfil de console, `session.bin`, arquivo de saída do plugin de captura.

Em texto versionado, chave aparece **mascarada**: quatro caracteres do começo,
quatro do fim, e o caminho do arquivo onde o valor real está.

```
hash_key   23fb...2d2f      (valor completo em dumps/console/sec_keys_v25.txt)
```

Exceções, e só estas:

- a 1BL key (`DD88AD0C...`) é pública e documentada desde o free60; pode ser
  citada inteira;
- hash (SHA-1, SHA-256) não é segredo: pode ser citado inteiro;
- chave pública pode ser citada inteira.

## Se um segredo entrar

Considere vazado. O histórico do git é copiável, e `git rm` não apaga nada.

1. Trate a exposição como definitiva para quem já clonou.
2. Troque o que for trocável. Chave de DRAM morre no próximo boot; CPU key não
   morre, então a decisão de continuar usando o console é consciente.
3. Só então limpe o arquivo. Reescrever o histórico é possível, mas não desfaz a
   exposição.

## Dados do console

- `dumps/` e `samples/` ficam fora do git, sempre. O `.gitignore` cobre; confira
  com `git status` antes de commitar.
- Captura com chave não vai para issue, pull request, chat ou print de tela.
- Antes de publicar log, procure chave e caminho de máquina dentro dele.

## Rede

- Nenhum teste, build ou experimento se conecta ao Live.
- Experimento com plugin no console: cabo de rede desconectado. Autenticar com
  identidade errada é o que bane console.
- O emulador não fala rede até o M7. Quando falar, é decisão registrada em
  `docs/decisions.md`.

## Terceiros e licença

- Nada da Microsoft no repositório: firmware, kernel, dash, keyvault, chave.
  Tudo isso vem da NAND do dono, em `dumps/`.
- `tools/build/xenon-bltool/`: só os headers de compatibilidade, GPL-2, com
  crédito no README do diretório.
- `tools/data/`: tabelas do Xenia, BSD-3, com crédito.
- Referência estrangeira serve para derivar chave e formato. Copiar
  implementação não.
- Toda citação de terceiro diz a licença e o que foi usado.

## Dependências

- Crate nova ou pacote Python novo precisa de justificativa na mensagem de
  commit e de licença compatível.
- `Cargo.lock` é versionado: build reproduzível não depende de resolução
  flutuante.
- Não misture atualização de dependência com mudança de comportamento.
- Dependência que baixa binário em tempo de build ou de execução é suspeita por
  padrão; se for inevitável, registre o motivo no commit.

## Arquivos que executam

- Script do repositório não baixa código da rede para rodar.
- CI não imprime segredo e não acessa `dumps/`: os testes de cadeia ficam
  `#[ignore]` justamente por isso.
- Antes de rodar script de terceiro baixado para `tools/stage3` ou
  `tools/stage4`, leia o que ele faz. Essas pastas são ignoradas pelo git e não
  são auditadas por ninguém.

## Verificação

[checklist.md](checklist.md), seção 9. Rode antes de cada commit que toque em
configuração, CI, `.gitignore` ou qualquer arquivo com número hexadecimal
grande.
