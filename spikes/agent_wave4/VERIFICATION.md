# Verificação da Onda 4

## Recorte

- Base aprovada: `98d0db1`.
- Implementação paralela em dois agentes, com arquivos de domínio separados e integração central dos arquivos compartilhados.
- Interfaces confirmadas para TDD: projetos/conversas nativos e avaliador puro de autorização.
- Escopo atual: macOS, acesso a arquivos validado por operação; distribuição/assinatura/App Sandbox adiados.
- Nenhuma autorização para incluir alterações anteriores do usuário no commit ou fazer push.

## Evidências focais até a integração

- Projetos: 15 testes focais aprovados pelo agente, com ciclos vermelho → verde relatados para cadastro, duplicidade, renomeação, disponibilidade, conversas opcionais, relocalização e seleção obsoleta.
- Política: 25 testes determinísticos aprovados pelo agente, com matriz de aprovação, escopo, contexto, modo, operação, destruição e rede.
- Seleção/projeção nativa: 3 testes de integração aprovados, incluindo cancelamento, URI/caminho inválidos e contrato IPC camelCase.
- Typecheck TypeScript aprovado durante a integração.

O teste do seletor usa a saída do callback como entrada controlada e não abre um diálogo real. Os testes de política não executam comandos, rede ou arquivos e não constituem prova de capacidades de runtime.

## Limites

- A UI de projetos não foi construída; comandos e cliente foram disponibilizados para FUI-027.
- Não foi aberto o aplicativo Tauri nem o banco real da aplicação.
- Incompatibilidade de path não UTF-8 é recusada; o volume do host não permitiu criar a fixture não UTF-8 e esse cenário não é marcado como prova executada.
- Disponibilidade em volumes não suportados e outras plataformas não foi exercitada.
- A autorização persistente, uso único de aprovações e despacho seguro ainda dependem de FUI-012/FUI-013/FUI-014/FUI-016/FUI-017.
- Os fatos de política devem ser resolvidos pelo backend, com identidade/versão/escopo do recurso verificados. Strings ou flags de um cliente não substituem essas garantias.
- Testes da aplicação usam o checkout de trabalho com alterações anteriores preservadas; isso será distinguido da compilação do patch isolado.

## Revisão e suíte final

Após as correções, o integrador executou a suíte completa e validou o patch preparado isolado das alterações anteriores do checkout.

| Verificação final | Resultado |
|---|---|
| `cargo test --offline --manifest-path src-tauri/Cargo.toml` | 222 testes unitários + 5 de integração aprovados; 0 falhas; 8 testes previamente ignorados |
| Casos focais novos incluídos | 17 de projetos, 32 de política e 5 de seleção/projeção/erros nativos |
| `pnpm test:smoke` | Build e 12 smoke tests aprovados |
| `pnpm typecheck` | Aprovado durante a integração |
| Regressões da Onda 1 | 21 aprovadas, sem sandbox pulado |
| Patch preparado sobre `git archive 98d0db1`, em diretório temporário | Compila e passa `cargo clippy --offline --lib -- -D warnings`, sem incluir alterações anteriores do usuário |

A coluna de geração do vínculo integra o esquema/stamp existente. Não foi adicionada migração: a próxima abertura real da aplicação segue a política de recriação autorizada para esquemas incompatíveis. Nenhum banco real da aplicação foi aberto por esses testes.

Os dados positivos de capacidade usados pela política continuam sendo fixtures sintéticas, não uma aprovação de isolamento de processos ou rede em produção.

### Standards

Revisão independente encontrou três observações de julgamento, corrigidas e revalidadas:

1. Erros de domínio eram achatados em `Storage`: agora possuem códigos estáveis e distintos, com teste público de serialização.
2. Seleção obsoleta podia passar após A→B→A: agora há `binding_revision` monotônica, comparação atômica e regressão vermelho → verde; rótulo/no-op não incrementam a geração.
3. DTO podia misturar caminho antigo e disponibilidade nova: agora a projeção deriva do próprio `ProjectRecord`, com regressões de snapshot no módulo e no adaptador IPC.

**Resultado: 0 violações documentadas e 0 achados remanescentes.** Revisores não executaram a suíte completa; os resultados acima são do integrador.

### Spec

Revisão independente encontrou P1 na restrição global de leitura, aplicada inicialmente apenas à operação de arquivo. Foi corrigido em paralelo com as demais correções: `ReadEvidence` cobre leituras efetivas da ação, incluindo comandos que também escrevem e uploads. Evidência desconhecida/incompleta ou leitura globalmente proibida bloqueia, mesmo diante de aprovação de comando, isolamento, modo automático ou autorização de rede.

Regressões negativas foram observadas vermelho → verde; um comando comprovadamente sem leitura continua podendo ser avaliado como escrita pura. Revisão também confirmou a geração de vínculo, a projeção coerente e os códigos de erro do IPC.

**Resultado: 0 defeitos remanescentes identificados e nenhum bloqueio de escopo para FUI-009/FUI-011.** Resolver persistente, consumo de aprovações, validação no despacho, UI e execução continuam nos seus tickets próprios.
