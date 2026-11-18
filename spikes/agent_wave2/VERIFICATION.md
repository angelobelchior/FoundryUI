# Verificação da Onda 2

## Recorte

- Ticket: FUI-006, consolidação documental do gate macOS.
- Base acordada com o usuário: `090bc1c01d0672f7598bff0bcf2412a81c7e5f22`.
- Comparação WIP: `git diff --cached 090bc1c01d0672f7598bff0bcf2412a81c7e5f22 -- spikes/agent_wave2`.
- Nenhuma alteração na implementação da Onda 1, no aplicativo, no banco ou em suas configurações foi necessária.
- As alterações anteriores do checkout foram preservadas e não integram a revisão nem o commit desta onda.

## Verificações executadas

| Verificação | Resultado observado nesta onda |
|---|---|
| `pnpm typecheck` | Aprovado |
| `WAVE1_SANDBOX_BACKENDS=macos python3 -m unittest discover -s tests -v`, em `spikes/agent_wave1` | 21 aprovados, 0 falhas, sem provas de sandbox puladas |
| `cargo test --offline --manifest-path src-tauri/Cargo.toml --lib` | 139 aprovados, 0 falhas, 8 testes previamente ignorados |
| `pnpm test:smoke` | Build aprovado; 12 smoke tests aprovados |
| Validação documental | 16 IDs de capacidade únicos, referências de evidência válidas, seletores de testes existentes, links locais e anchors resolvidos |

Não houve nova execução de inferência real nesta onda. A evidência Qwen/Foundry vem da Onda 1 e mantém suas condições e limites explícitos. A repetição de fixtures valida regressão do harness, não cria nova evidência de compatibilidade de modelo ou runtime.

Os testes da aplicação foram executados no checkout de trabalho, que inclui as alterações anteriores do usuário. Seus resultados não equivalem a uma alegação de reprodução do aplicativo em um checkout limpo do commit-base. O harness da Onda 1 estava sem diferenças locais em relação à versão referenciada.

O teste de descendentes que chama `setsid()` passou ao reproduzir uma insuficiência. Ele mantém o candidato de comandos reprovado; não aprova a garantia de cancelamento.

## Standards

Revisão independente dos documentos principais: 0 violações documentadas e 0 achados de julgamento. Foram verificadas separação de evidência/proposta, origem dos dados, referências e preservação dos bloqueios. Nenhum teste ou serviço foi iniciado pelo revisor.

## Spec

Revisão independente concluída: 0 achados, em todas as severidades. O revisor confirmou que o bloqueio explícito da fatia vertical atende à alternativa prevista por FUI-006, as evidências permanecem limitadas ao ambiente provado, o candidato de comandos reprovado não foi promovido e os contratos não se apresentam como implementação de produção.

As lacunas GAP-01–07 e o adiamento de Windows/Linux foram preservados. A revisão foi documental, sem executar testes, serviços ou inferência; os resultados de execução acima pertencem ao processo de verificação desta entrega, não aos revisores.

**Resumo da revisão: Standards 0 achados; Spec 0 achados.**

## O que não foi habilitado

- Nenhuma ferramenta de arquivos, edição, comandos ou rede do agente foi registrada no aplicativo.
- Não existe novo módulo de produção de capability gate nesta onda; sua interface e invariantes foram especificadas para os tickets responsáveis.
- Nenhum valor “suportado” na matriz concede autorização de produção ou ultrapassa os limites da evidência.
- Nenhum item de Windows/Linux foi executado ou declarado suportado.

Os estados de FUI-006 e da lista de ondas ficam no diretório externo `FoundryUI-Specs`, atualizado separadamente e fora deste repositório, como solicitado pelo usuário.
