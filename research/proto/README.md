# Proto (`Proto.xex` + `Proto.ini`)

Plugin de HUD/rede da comunidade. Roda como plugin de kernel no RGH.

## Build em uso no console

- `Proto.xex` 151.552 bytes, sha256 `c656ea9f0ed14bef24536a691a68efefc005f40ae7f0e4383ba991aea187d15c`
- XEX2 sem cifra, LZX; imagem `0x5F000` (389.120 bytes), base `0x91850000`, entrada `0x91873910`
- 357 imports: 190 de `xam.xex` e 167 de `xboxkrnl.exe`, em 181 slots ordinais e 176 thunks de chamada

O `.text` e o `.rdata` do arquivo não são o código final: o plugin se desempacota
em memória. A captura pós-carga em `dumps/console/CAP00/proto.mem` mostra o
resultado — `.pdata`, `.data` e o stub de entrada batem com a imagem do XEX; o
resto de `.text`/`.rdata` é o payload já desempacotado. Os 357 imports da
captura estão resolvidos e servem de gabarito para o loader.

## Outros builds

`samples/Proto.xex` é um build anterior, cifrado: 247.808 bytes, sha256
`50e1e9959c71cc717f757416ca805a46642115c75984db7e6df657cca7d1d0e5`, entrada
`0x91883800`, 271 imports (116 `xam.xex` + 155 `xboxkrnl.exe`). É o arquivo que o
teste de extração cifrada usa.

O `Proto.ini` do console (`9a5afe4c…a7795`, 912 bytes) tem a seção
`[Enforced DNS]` preenchida com `1.1.1.1`/`1.0.0.1`; o de `samples/` é o padrão
distribuído.

## O que o código faz (por imports)

| Função | Evidência |
|---|---|
| Altera o HUD/guide | `XuiRegisterClass`, `XuiElementSetPosition`, `XuiResource*`, `XuiSendMessage`, `XNotifyQueueUI`, `XamShowCustomPlayerListUI` |
| Rede própria (TCP) | `NetDll_socket/connect/send/recv/setsockopt`, `XNetGetEthernetLinkStatus` |
| Identidade e Gold | `XamUserGetSigninInfo/State`, `XamUserGetMembershipTier`, `XamGetCurrentTitleId`, `XamGetExecutionId` |
| Ids do console | `ExGet/SetXConfigSetting`, `XeKeysUnObfuscate`, `HalSendSMCMessage`, `XboxHardwareInfo` |
| Criptografia | `XeCryptSha`, `HmacSha`, `Rc4`, `XeCryptBnQwNeRsaPubCrypt`, `XeCryptBnDwLePkcs1Verify` (verifica assinatura RSA, provavelmente de respostas do servidor) |
| Carrega plugins e dash | `XexLoadImage`, `XexGetProcedureAddress`, `XamLoaderLaunchTitleEx`, `XamReloadSysApp`, `ObCreateSymbolicLink` |
| Downloads em jogo | `XamBackgroundDownloadSetMode` |
| Tarefas | `ExCreateThread`, `XamTaskSchedule` |

## `Proto.ini`

`protoHud`, `disableHudRestriction`, `customDash`/`customDashPath` (só devkit, não muda o boot), `disableOnlinePresence` (presença é entre usuários Proto), `backgroundDownloadPatch`, `[Plugins] plugin1..5` (só plugins ao lado do `Proto.xex`).

## Limites

- `.rdata` tem blobs ofuscados e quase nenhuma string legível: **endpoints, portas e protocolo não saem de análise estática simples.** Não tentei desofuscar.
- Para saber com quem ele fala: capturar o tráfego no RGH real com o Proto carregado (ver o marco de Live em [`docs/status.md`](../../docs/status.md)).

## Requisitos para o emulador

Pilha de rede (`NetDll_*`), `XNotify`/XUI do `xam` real e o loader de plugins do
emulador (`[Plugins]` do `launch.ini`, ver [plugins.md](../../docs/plugins.md)).
O loader já extrai a imagem e classifica os imports; mapear em RAM e chamar a
entrada dependem do kernel. Teste de aceitação: o HUD do Proto aparece sobre a
dash emulada.
