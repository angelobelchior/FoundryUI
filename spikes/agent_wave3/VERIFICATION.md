# Verificação da Onda 3

## Escopo e proveniência

- Base acordada: `dd28a2f`, somente os módulos e trechos de integração escritos nesta onda.
- Interfaces de testes confirmadas: persistência e recursos locais, observadas por operações públicas.
- Plataforma de recursos: macOS ARM64 do ambiente atual. Windows/Linux continuam adiados.
- O usuário confirmou contenção estrita; acesso por objeto já aberto não foi aceito como substituto.
- A revisão e o commit devem excluir as alterações anteriores nos arquivos compartilhados, especialmente `db.rs` e `schema.sql`.

## TDD e testes focais

Foram observados ciclos vermelho → verde para abertura idempotente de execução, histórico estruturado/sanitizado, conflito de IDs, decisões/claim/resultado, reinício, snapshots, revisão contextual, fatos tipados, projetos e diário ordenado. No módulo de recursos, os ciclos cobriram identidade/aliases, ancestralidade, substituição/revogação e negação de I/O sem garantia estrita.

Regressões adicionais exercitam concorrência com barreira, falha de metadados de resultado, referências entre conversas, reabertura de resultados concluídos, case do filesystem, symlink retarget, falta de permissão e pai movido. Os dados e alvos são fixtures temporárias.

Nenhum teste de criação/leitura bem-sucedida com contenção estrita foi apresentado como aprovado. Testar a negação desses métodos prova a postura fail-closed atual, não implementa a funcionalidade positiva pendente.

## Verificações durante a implementação

- Typecheck TypeScript executado regularmente e aprovado.
- Testes focais de persistência e recursos aprovados nos comandos executados até aqui.
- `cargo clippy --offline --manifest-path src-tauri/Cargo.toml --lib -- -D warnings` encontrou um `clippy::collapsible_if` já existente em `src-tauri/src/foundry/chat.rs:857`, em trecho anterior do usuário que não integra esta onda. Não foi corrigido nem descrito como aprovação de Clippy.
- O patch preparado foi aplicado a um checkout temporário extraído de `git archive dd28a2f`, sem alterações anteriores do usuário, e compilou com `cargo check --offline`.
- Após as correções, o mesmo processo de isolamento executou `cargo clippy --offline --lib -- -D warnings` com sucesso sobre o patch preparado. O alerta do checkout do usuário não foi suprimido nem alterado.

## Verificação final executada

| Verificação | Resultado |
|---|---|
| `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib` | 167 aprovados, 0 falhas, 8 testes previamente ignorados |
| Testes Rust novos incluídos nesse total | 20 de persistência e 8 de recursos no macOS |
| `WAVE1_SANDBOX_BACKENDS=macos python3 -m unittest discover -s tests -v` | 21 aprovados, sem sandbox pulado |
| `pnpm test:smoke` | Build e 12 smoke tests aprovados |
| `pnpm typecheck` | Aprovado durante os ciclos da implementação |
| Compilação do índice isolado | Aprovada sobre `dd28a2f` mais o patch preparado, sem alterações anteriores do usuário |
| Clippy do índice isolado | Aprovado com `-D warnings`, sem ignorar o alerta do checkout de trabalho |

O teste de contenção por `setsid()` da Onda 1 continua sendo prova negativa do candidato de comandos. Os testes novos de I/O estrito negado também não são convertidos em prova positiva de acesso/criação.

## Limites dos resultados

- Testes da aplicação usam o checkout de trabalho, que preserva alterações anteriores do usuário; não equivalem a um checkout limpo do commit-base.
- A camada de dados grava fatos e estados; não executa modelos, comandos, requisições HTTP nem arquivos de trabalho.
- Os fatos de rede são normalizados sem realizar transporte; esses testes não comprovam um mediador HTTP de produção.
- Identidade/ancestralidade são metadados observados, não uma licença durável de leitura após movimentação ou revogação.
- Volumes removidos à força, todos os sistemas de arquivos e versões macOS não exercitadas não são apresentados como cobertos por mocks ou por sucesso no APFS local.

## Standards

Revisão inicial: 0 violações documentadas; 2 observações de julgamento (P2, sanitização de chaves dinâmicas; P3, perda de proveniência dos erros SQLite). Ambas foram corrigidas e revalidadas pelo revisor, sem bloqueio novo.

- Chaves desconhecidas passam por sanitização, com regressão pública observada em vermelho e verde para argumentos, resultados e snapshots.
- Erros preservam `StorageFailureKind` e código SQLite numérico, sem payloads. Uma conexão SQLite independente é usada apenas para manter uma transação de escrita como fixture de contenção; as asserções observam o erro da interface pública, sem consultar esquema ou estado privado.
- A centralização da sequência evita perder a classificação ao passar pelo módulo antigo do banco.

**Standards: 0 achados remanescentes.**

## Spec

Revisão inicial: 2 defeitos P1 (chaves sensíveis e exclusão que apagava a barreira de escritor incerto). Ambos foram corrigidos com regressões públicas vermelho → verde e revalidados.

A exclusão agora obtém transação IMMEDIATE, verifica trabalho ativo/incerto e só então remove registros. Testes cobrem claim em execução, claim interrompido após reabertura e exclusão positiva de trabalho concluído, preservando a cópia independente em outra conversa.

**Spec: 0 defeitos de implementação remanescentes identificados; 1 requisito conhecido bloqueado (FUI-008, acesso/criação positivos com contenção estrita).** Esse impedimento permanece nos critérios e nas dependências; não houve fallback por objeto fixado.
