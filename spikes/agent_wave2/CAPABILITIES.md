# Matriz de capacidades e impedimentos

## Como interpretar

**Estado da prova** tem três valores:

- **Suportado:** o comportamento limitado da linha foi observado no experimento e ambiente identificados.
- **Não suportado:** existe contraexemplo ou o candidato/operação foi explicitamente rejeitado para o contrato exigido.
- **Não comprovado:** não há evidência suficiente, mesmo que o catálogo declare suporte ou uma parte do fluxo funcione.

Esses estados não são autorização de produção. **Todas as capacidades novas do agente continuam sem aprovação de produção nesta onda.** O suporte experimental de uma parte não torna seu composto suportado. “Adiado” indica recorte de trabalho e é acompanhado de “não comprovado”, não de suporte implícito.

Salvo indicação contrária, as linhas observadas referem-se a macOS 27.3/Darwin 27.3.0 ARM64, Python 3.15.2, código no commit `090bc1c01d0672f7598bff0bcf2412a81c7e5f22`.

## Evidências identificadas

| ID | Fonte | Alcance |
|---|---|---|
| EV-01 | [Resultado real e contrato de protocolo](../agent_wave1/reports/macos-findings.md#fui-001--protocolo-e-modelo) | Ciclo Qwen/Foundry real; não envolve arquivos do usuário |
| EV-02 | [ToolStreamTests](../agent_wave1/tests/test_protocol.py) | Fixtures de wire, fragmentação, interleaving, erros e validação atômica |
| EV-03 | [MacSandboxTests](../agent_wave1/tests/test_sandbox.py) | Acesso permitido, arquivos/symlinks negados, ambiente e sockets de controle |
| EV-04 | [Prova negativa de lifecycle](../agent_wave1/reports/macos-findings.md#prova-negativa-de-lifecycle) e `MacSandboxTests.test_detached_descendant_demonstrates_group_kill_is_not_containment` | Escrita finita posterior ao encerramento do grupo original |
| EV-05 | [MediatedClientTests](../agent_wave1/tests/test_network.py) | HTTP loopback temporário, aprovações, redirects, limites e sanitização |
| EV-06 | [LocalModelProbeTests](../agent_wave1/tests/test_model.py) | Contrato de duas chamadas contra fixture HTTP; não é modelo real |
| EV-07 | [Limites de distribuição macOS](../agent_wave1/reports/macos-findings.md#distribuição-e-limites) | Interface privada/depreciada e cenários não comprovados |
| EV-08 | [Plataformas adiadas](../agent_wave1/reports/macos-findings.md#plataformas-adiadas) e [pesquisa documental](../agent_wave1/reports/windows-linux-research.md) | Windows não executado; experiência Linux limitada e fora do recorte atual |
| EV-09 | [Verificações finais da Onda 1](../agent_wave1/reports/macos-findings.md#verificações-finais-executadas) | Resultados históricos do harness versionado e das verificações do checkout, que preservava alterações anteriores não commitadas da aplicação |
| EV-10 | [Resultados e limites da rede mediada](../agent_wave1/reports/macos-findings.md#fui-005--rede-mediada) | GET em fixtures, limites do transporte e funcionalidades de produção ainda ausentes |

Cada seletor de teste citado é encontrado nos arquivos vinculados. Não há transcrição de credenciais, conteúdo de projeto ou stdout não sanitizado nos diagnósticos propostos.

## Matriz

| ID | Capacidade e ambiente específico | Estado da prova | Estado no produto | Evidência / requisito de ambiente / limite |
|---|---|---|---|---|
| CAP-01 | Montagem e validação de chamadas estruturadas no wire exercitado | Suportado | Não integrado; sem aprovação | EV-02; schemas fixos e stream limitado; erros invalidam o lote, sem execução de texto |
| CAP-02 | Qwen `qwen3-14b-generic-gpu:2`, `/no_think`, Foundry 0.13.0, ORT 1.31.0/GenAI 0.18.0 | Suportado | Não integrado; sem aprovação | EV-01; modelo em cache/carregado; endpoint HTTP literal de loopback; ferramenta fixa em memória |
| CAP-03 | Mesma variante Qwen sem `/no_think`, nos limites iniciais do spike | Não comprovado | Bloqueado para alegação de suporte | EV-01; tentativa inicial não completou a prova; não generalizar para incompatibilidade permanente |
| CAP-04 | Outros modelos, versões ou opções de geração | Não comprovado | Bloqueado para ações até prova aplicável | EV-01/EV-02; fixtures e metadados não comprovam novas combinações |
| CAP-05 | Ler/criar fixture dentro da raiz e negar acessos/symlinks externos exercitados com `sandbox-exec` | Suportado | Candidato de teste apenas; sem aprovação | EV-03; diretórios temporários controlados e exceções de runtime; sem prova geral de identidade/TOCTOU |
| CAP-06 | Negar conexão TCP loopback e Unix socket nas fixtures macOS | Suportado | Candidato de teste apenas; sem aprovação | EV-03; listeners alcançáveis pelo controle; não comprova todos os brokers/canais de IPC |
| CAP-07 | Encerramento de toda a árvore por `killpg` no candidato macOS | Não suportado | Reprovado e bloqueado | EV-04; descendente chama `setsid()` e escreve após timeout; `cleanup_verified` continua falso |
| CAP-08 | Executar comandos arbitrários usando esse candidato | Não suportado | Reprovado e bloqueado | EV-03/EV-04/EV-07; sucesso parcial de arquivos/rede não cobre lifecycle ou todos os escapes |
| CAP-09 | GET mediado em HTTP com IP literal de loopback, redirects e eventos sanitizados | Suportado | Protótipo apenas; sem aprovação | EV-05; servidor temporário controlado; sem proxy herdado; autorização reaplicada em cada salto |
| CAP-10 | HTTPS mediado como capacidade de produção completa | Não comprovado | Bloqueado até implementação/prova | EV-05/EV-10; código usa validação TLS padrão, mas não houve validação externa completa nem deadline/cancelamento/retenção de produção |
| CAP-11 | Rede direta/opaca de comandos ou protocolos não auditáveis | Não suportado | Bloqueado pela política | EV-05, NET-03/NET-05; falha de auditoria nunca libera terminal com rede irrestrita |
| CAP-12 | Leitura real autorizada pelo agente + proposta de edição + continuação em uma fatia integrada | Não comprovado | Bloqueado | EV-01/EV-03 são provas separadas; não existe teste integrado de autorização/identidade/proposta; diagnóstico `vertical_slice_unproven` |
| CAP-13 | Ausência de tráfego do próprio Foundry/SDK e de todo o host | Não comprovado | Não alegar garantia offline total | EV-01; startup informou telemetria e catálogo; mediação de ferramenta não audita o runtime nem aplicativos externos |
| CAP-14 | macOS 15 ARM64 e demais versões não exercitadas | Não comprovado | Sem aprovação por extrapolação | EV-07; executar provas no alvo ou demonstrar explicitamente cobertura de compatibilidade antes de habilitar |
| CAP-15 | Windows x64/ARM64 | Não comprovado; adiado | Fora da implementação atual | EV-08; sem prova nativa; pesquisa não é teste; usuário determinou foco macOS |
| CAP-16 | Linux desktop x64/ARM64 | Não comprovado; adiado | Fora da implementação atual | EV-08; experimento de contêiner não comprova distribuição desktop, sockets ou todas as arquiteturas |

Uma mudança de ambiente reabre a classificação pertinente. Não selecionar uma linha apenas pelo nome do modelo nem tratar a versão macOS como um prefixo genérico “Darwin”.

## Cobertura dos cenários referenciados por FUI-006

| Cenário | Evidência atual | Conclusão restrita |
|---|---|---|
| T11 — alcance e substituição de caminhos | EV-03: `MacSandboxTests.test_outside_files_and_symlinks_are_denied_and_credentials_not_inherited` | Casos limitados de arquivo externo e symlinks exercitados; matriz completa de identidade/races ainda pendente em FUI-008/FUI-034 |
| T12 — subprocessos, arquivos e sockets | EV-03 e EV-04 | Há negações reais de arquivos/sockets, mas o executor composto não foi aprovado; lifecycle reprovado |
| T13 — comando não recebe rede implicitamente | EV-03 e EV-05 | Rede negada nas fixtures e rede mediada separada; não há ferramenta de comandos de produção liberada |
| T16 — operação não auditável bloqueada | EV-05: `MediatedClientTests.test_unsupported_or_credential_bearing_urls_are_blocked_before_transport`; CAP-11 | Protocolos não suportados rejeitados; não existe liberação alternativa de processo opaco |
| T25 — incompatibilidade/chamada inválida | EV-02 e EV-06 | Chamadas inválidas são rejeitadas, sem extração de comandos de texto; experiência final de escolha de modelo ainda depende de FUI-029 |

Essa tabela não marca T11–T37 de produto como integralmente aprovados. Os testes completos da matriz original permanecem exigidos nas respectivas entregas.

## Lacunas obrigatórias

| ID | Impedimento | Responsabilidade / condição para resolver |
|---|---|---|
| GAP-01 | Não há encerramento macOS de escritores remanescentes comprovado | FUI-014/FUI-016: mecanismo de isolamento/lifecycle com prova de descendentes destacados e perda do launcher; manter recurso ocupado/estado incerto até comprovação |
| GAP-02 | A fatia de arquivo/proposta não foi demonstrada | FUI-008/FUI-011/FUI-012/FUI-013/FUI-015/FUI-023: compor identidade, autorização e propostas sem editar fora da revisão |
| GAP-03 | Origem do endpoint e configuração local do runtime não constituem prova offline total | FUI-018: verificar proveniência e política antes de enviar contexto; informar limites de tráfego do runtime |
| GAP-04 | Mediador ainda não tem persistência/retenção, deadline global ou cancelamento de produção | FUI-007/FUI-017: impor limites, registrar resultados reais por conversa e eliminar canais sem auditoria |
| GAP-05 | Versões/plataformas não testadas e combinações novas de modelos | FUI-013/FUI-016/FUI-034: evidência aplicável por ambiente; Windows/Linux continuam adiados até nova decisão de recorte |
| GAP-06 | Flags da interface/catálogo ainda não equivalem a autorização de efeitos | FUI-011/FUI-012/FUI-013: autoridade no backend, decisões vinculadas à ação/revisão e bloqueio no ponto de uso |
| GAP-07 | Edições de comandos e recuperação não foram implementadas | FUI-023/FUI-024/FUI-025/FUI-026: diferenças antes de aplicar, proteção Git real e preservação do trabalho do usuário |

Nenhuma lacuna é removida por marcar FUI-006 como concluído. A aceitação final continua bloqueada para capacidades cujas garantias não tenham sido demonstradas.
