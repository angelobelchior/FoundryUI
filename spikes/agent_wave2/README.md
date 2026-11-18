# Onda 2 — Gate de capacidades macOS

Ticket: FUI-006. Entrega: registro técnico para orientar as implementações seguintes, sem habilitar ações do agente no produto.

## Decisão

**Consolidar a investigação e prosseguir somente com fundações, mantendo as capacidades de produção sem aprovação.**

- A combinação Qwen/Foundry exercitada tem evidência de chamadas estruturadas e continuação, não de um agente completo sobre arquivos.
- O candidato `sandbox-exec` com encerramento por grupo está reprovado para executar comandos: descendentes em outra sessão podem continuar escrevendo.
- A mediação de GET foi exercitada em fixtures; não equivale a rede arbitrária de processos nem a um módulo de produção com retenção/cancelamento completos.
- A fatia vertical de leitura autorizada, proposta de edição e continuação do modelo **não foi comprovada**. Seu estado é bloqueado, não concluído por analogia com a fixture em memória.
- Windows e Linux permanecem adiados conforme a decisão do usuário. Nenhum suporte nessas plataformas é inferido.

Concluir FUI-006 significa concluir esta decisão e seus contratos, usando a alternativa de bloqueio explícito prevista no ticket. **Não significa aprovar a fatia vertical ou os gates de segurança para produção.**

## Artefatos

| Documento | Conteúdo |
|---|---|
| [Matriz de capacidades](CAPABILITIES.md) | Estados de suporte no ambiente provado, evidências, limites e lacunas de aceite |
| [Contratos propostos](CONTRACTS.md) | Interfaces de modelo, isolamento, rede e diagnóstico; imposição futura no backend |
| [Registro de verificação](VERIFICATION.md) | Testes executados nesta onda, proveniência e revisão |
| [Evidências da Onda 1](../agent_wave1/reports/macos-findings.md) | Ambiente, ciclo real, provas positivas/negativas e limitações |
| [Revisão da Onda 1](../agent_wave1/reports/review.md) | Achados corrigidos e revalidação dos protótipos |

Os documentos desta onda são propostas técnicas e decisões de habilitação. Não são uma implementação do módulo de autorização e não devem ser carregados como permissões executáveis.

## Origem e atualidade da evidência

- Snapshot de implementação: commit `090bc1c01d0672f7598bff0bcf2412a81c7e5f22`.
- Ambiente da prova real: macOS 27.3/Darwin 27.3.0, ARM64; Foundry 0.13.0; ORT 1.31.0; ORT GenAI 0.18.0.
- Modelo: `qwen3-14b-generic-gpu:2`, já em cache, com soft switch explícito `/no_think`.
- Os testes e suas fontes estão versionados em `spikes/agent_wave1`. O resultado com modelo real é histórico e identificado; uma reexecução de fixtures nesta onda não será apresentada como nova inferência real.
- Nenhuma evidência se transfere automaticamente para outro modelo, opção, runtime, versão do sistema, arquitetura ou mecanismo de isolamento.

## Fatia vertical: bloqueio explícito

| Passo pretendido | O que existe | O que impede afirmar sucesso |
|---|---|---|
| Autorizar e ler um arquivo solicitado pelo modelo | Leitura em fixture de sandbox e consulta independente de um valor fixo pelo modelo | Não há integração entre autorização contextual, identidade de recurso, leitura atual e chamada do modelo |
| Propor uma edição sem modificar a pasta real | Critérios e contrato de propostas definidos nas especificações | Ainda não existe o módulo de propostas com versão-base, revisão e aplicação controlada |
| Continuar usando resultado correlacionado | Ciclo real da ferramenta fixa em memória, com `tool_call_id` preservado | A prova não inclui o resultado de uma leitura autorizada real nem uma proposta de alteração |
| Executar comandos e parar sem escritores remanescentes | Candidato restritivo de arquivos/rede e prova negativa de lifecycle | `setsid()` permite sair do grupo encerrado; limpeza da árvore não foi comprovada |

Não será criada uma leitura direta de arquivo fora da política para produzir uma demonstração artificialmente verde. Enquanto os módulos necessários não existirem, o diagnóstico correto é `vertical_slice_unproven`.

## O que pode avançar após esta decisão

- FUI-007: persistência e histórico estruturado, preservando a distinção entre ação proposta, autorizada, iniciada e concluída.
- FUI-008: identidade e acesso efetivo a recursos, sem tratar `realpath` isoladamente como proteção contra substituição concorrente.
- Contratos e testes dos tickets posteriores podem ser preparados conforme suas dependências, sem habilitar ferramentas de efeito antecipadamente.

FUI-011/FUI-012 implementam política e aprovações; FUI-013, o ciclo do modelo; FUI-014/FUI-016, lifecycle e comandos; FUI-017/FUI-018, rede e origem do endpoint; FUI-023/FUI-024/FUI-026, propostas, recuperação e efeitos de comandos. A aceitação dessas capacidades depende de suas próprias provas, não do estado `done` deste registro.

## Regra de habilitação

O backend deve bloquear uma ação se faltar qualquer uma destas condições:

1. Implementação de produção disponível para a operação.
2. Evidência positiva aplicável ao ambiente efetivo e a todas as garantias necessárias.
3. Política contextual vigente permitindo a ação, com aprovações obrigatórias ainda válidas.
4. Recursos atuais correspondentes aos alvos aprovados.
5. Execução/lifecycle em estado que permita o efeito com segurança.

Qualquer negação obrigatória prevalece sobre flags do catálogo, preferências do usuário, modo automático, aprovação antiga ou instrução do modelo. Evidência apenas experimental não satisfaz a primeira condição. Detalhes e diagnósticos estão em [CONTRACTS.md](CONTRACTS.md).

## Verificação desta onda

Esta entrega não adiciona comportamento executável, parser, ferramenta ou nova seam de testes. TDD não é aplicável a uma nova implementação nesta onda; as três seams já acordadas da Onda 1 permanecem como superfície de regressão.

Foram verificados links, anchors, seletores de testes, consistência da matriz com o snapshot de origem e ausência de habilitação de capacidades. As regressões existentes passaram: 21 testes do harness, 139 testes Rust (8 previamente ignorados), 12 smoke tests, typecheck e build. As revisões independentes de Standards e Spec terminaram sem achados.

Os resultados e limites de reprodução estão em [VERIFICATION.md](VERIFICATION.md). Não houve nova inferência real nesta onda nem conversão de prova negativa em aprovação. A consolidação de FUI-006 está concluída; as capacidades de produção permanecem sem aprovação conforme a matriz.
